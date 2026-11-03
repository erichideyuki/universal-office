# uo-docx — the Write document engine (model, reader, lossless writer, editing session, Editing API, `.doc`, exporters)

Record of the rust-port workflow. Status: **milestone-1 gate approved 2026-11-03** (hybrid mode, `SKILL.md` "Hybrid mode":
milestone 1 = the read path, uo-docx D1a → uo-docx-layout → uo-render → uo-pdf). Planner and reuse
analysis together (one Opus agent), then the analysis skeptic (section "Analysis skeptic", kept as
written), then this revision. *(revised after the skeptic)* Under the hybrid mode there are no
gate-1b briefs for the Sonnet parts: the architecture below closes the shapes, the Sonnet frontiers
port 1:1 inside it and declare every decision outside the source semantics in this record, and
verification runs continuously. Opus writes a short design note only where the verdict table says
Opus (D1b core, D2a, find/paste, D3 DOM, `odraw` shape).

All line counts below are non-test source lines on `main` 0.9.x (frozen), counted in this worktree.

## Scope

### Behaviour sources

| Source | Lines | Content |
|---|---|---|
| `write/packages/docx-core/src/**` | 24,653 (72 files) | model types and readers (`document.ts` 2,312, `fulldoc.ts` 868, `ooxml/props.ts` 821, `ooxml/parts.ts` 576, `drawing.ts` 600, …), the lossless writer (`serialize.ts` 1,821, `lossless.ts` 599, `source.ts` 331, `rev-xml.ts`, `table-xml.ts`, `settings-xml.ts`, `hf-ops.ts`, `notes.ts`, `page-background.ts`, `watermark.ts`), edit operations (`model-ops.ts` 455, `table-ops.ts` 1,198, `section-ops.ts` 213, `revisions.ts` 566, `style-sheet.ts` 427, `picture-xml.ts` 1,057, `shape-xml.ts` 607, `table-sort.ts`, `table-formula.ts`, `table-style-edit.ts`), `export/` 623, `odf/` 1,700, `doc/` 6,017 |
| `write/packages/editor/src/universal-write.ts` | 23,473 | one custom element with ≈ 1,140 methods (532 public). Classified by a script over method bodies (DOM/canvas/event tokens vs model tokens): model-only 7,081 lines, mixed 6,699, DOM-only 3,504, neither 4,577 (mostly the legacy canvas layout and its PDF/XPS painters). **≈ 10–11k lines of model behaviour move** (the model-only methods and the model half of the mixed ones, incl. undo `stringifyState`/`popHistory` 9,406–9,680, `installFullDoc` 2,340, `saveDocxBody` 8,546, the 141 `snapshot()` call sites, and *(revised after the skeptic, U1)* the model mutations of the layout pass: `syncSections`, `syncRunRPr`, `refreshFieldCache`/`setFieldResult`); ≈ 12.5–13k stay as the shell |
| editor model files (pure, no DOM) | ≈ 6,000 | `numbering-store.ts` 325, `styles.ts` 541, `references.ts` 525, `notes.ts` 160, `revisions.ts` 143, `find-engine.ts` 562, `toc.ts` 223, `paste-html.ts` 323 (rules; DOM parsing stays), `engine-doc.ts` 465 (`reconcileBlocks`, bookmarks; caret helpers go to layout), `picture-ops.ts` 391, `shape-ops.ts` 187, `text-case.ts`, `edit-guard.ts` 87, `navigation.ts` 211, `text-layout.ts` 222, `page-design.ts` 106, `formatting-policy.ts` 546, data: `hf-templates.ts` 260, `cover-templates.ts` 231, `list-library.ts` 331 (generated), `text-effects-presets.ts` 132 |
| `write/packages/compare/src/**` | 1,756 | document comparison (Myers/histogram diff over blocks, intra-word marks); *(revised after the skeptic, U20)* of which `universal-write-diff.ts` (702) is a shell element and stays TS: **1,054 move** |
| Moved in from other records | 513 | `shared/fonts/src/ooxml.ts` embedding read/write (uo-fonts G2, 345, with its regex spec table); `shared/drawingml` `contour.ts` 80 + `shape-styles.ts` 88 (uo-drawingml G3) |

### C# (type guide; what exists only in C# moves too)

- **C#-only, moves**: `dotnet/src/UniversalOffice.Docx/Editing/**` (4,699 lines, 26 files: `DocxDocument`
  with strict System.Xml reading and its own serializer, `DocxBodyWriter`, `DocxSanitizer`,
  `DocxTemplate` (empty / apply content / sections), `DocxFields` (allow-list, `ConvertHyperlinkFields`),
  `DocxCustomInlines`, `DocxText` (replace, `#{param}`), `DocxShapes`, `DocxPictures` + `ImageInfo`,
  `DocxNumbering`, `DocxStyles`, `DocxSections`, `DocxProperties`, `DocxReview`, `DocxImporter`,
  `DocxParagraphSamples`, `DocxTableRowTemplate`, `DocxStructure`, `DocxLimits`), plus the
  top-level `CustomInlineTag.cs`. Its test suite (`dotnet/tests/UniversalOffice.Tests/Editing`, 3,249
  lines) is the behaviour spec. The clickCompliance host uses exactly this surface
  (`DocxSanitizer` with input sanitization and its `IsCustomInlineValid` callback, `DocxDocuments.Open`
  with `DocxLimits`, `DocxFields.ConvertHyperlinkFields`, `CustomInlineTag`, `DocxBodyWriter`,
  `DocxParagraphSamples`, the template emptying).
- **Ports of the TS (type guide only)**: `DocumentReader.cs`, `Model.cs`, `FullDoc.cs`, `Serialize.cs`
  (a subset), `Drawing.cs`, `PictureXml.cs`, `Ooxml/*`, `DocBinary/*` (7,030), `Export/*`
  (`DocxMarkdown`/`DocxHtml`/`FromDocx`). `Export/DocxPdf.cs` and `DocxLayout/*` (18,797) belong to
  uo-docx-layout/uo-pdf.

### Left for other modules

- `uo-docx-layout`: layout (`docx-layout` engine, para, table, lines, context, resolve, fonts,
  geometry, math, numbering render, hyphenate, outline, underline, revisions marks, frames,
  `field-formula`, `datefmt`, the 600-DPI measuring model), the caret/hit-test helpers of
  `engine-doc.ts` (`caretXIn`, `offsetAtX`, `locateRef`, `findItems`), the editor's incremental
  relayout (`tryFastLayout`/`tryFastTable`/`installEngineResult`), gallery/TOC/style previews, the
  `LayoutOracle` implementation *(revised after the skeptic, U1, U10)*, font usage, and the
  web/draft views (G19).
- `uo-render` (display list), `uo-pdf` (writer + painter), `uo-image` (codecs, EMF/WMF), XPS export
  (`editor/src/xps.ts` + `exportXps`).
- `uo-drawingml`: `apply_color_mods` (stage 3 now), plus the colour resolver and theme reader
  **designed here** (G12) and the msospt map and OfficeArt records **placed there** by this plan (G11).

### Stays in the TS shell

Input (keyboard, IME, pointer, drag), the hidden textarea, DOM events (re-dispatched from the
session's events), chrome (`chrome-*.ts` ≈ 14.4k), canvas painting of the display list,
scrolling/zoom, dialogs, `DOMParser` and the inline CSSOM for pasted HTML (G15), image decode/encode
through the browser codecs (G25), `FontFace` loading and Local Font Access, spellcheck (Hunspell
worker; Rust gives text and gets ranges back), the OLE spreadsheet editor bridge, plugins registry
and plugin inline renderers (`draw`; `measure` is a layout callback), policy-driven UI hiding, the
`<universal-write-diff>` element.

## Plan

### The split

One crate `uo-docx` (crate rule: one app, one binding boundary), modules behind cargo features so
hosts link only what they use. Seven sub-modules. *(revised after the skeptic: model column from the
skeptic's verdict; D2b's dependency on the `LayoutOracle`; D5 lines; hybrid-mode milestones.)*

| # | Sub-module (module, feature) | Behaviour source | Lines | Port model | Depends on | Milestone |
|---|---|---|---|---|---|---|
| **D1a** | model + reader (`model`, `read`, `formatting`) | `document.ts`, `fulldoc.ts`, `ooxml/props.ts`, `ooxml/parts.ts`, `drawing.ts`, `source.ts` (reader side), `styles.ts`, `table-styles*.ts`, `word-styles.ts`, `style-names-es.ts`, `text-effects.ts`, `vml-shape.ts`, `fonts.ts`, `inspect.ts`, `blank.ts`, read parts of `page-background.ts`/`watermark.ts` | 6,800 | types **Opus** (done in "Architecture (milestone 1)"); reader **Sonnet** frontiers R1–R6 | uo-opc, uo-jsnum A+B, uo-fonts (odttf), uo-drawingml (resolver, theme) | **1** |
| **D1b** | lossless writer (`write`) | `serialize.ts`, `lossless.ts`, `rev-xml.ts`, `table-xml.ts`, `settings-xml.ts`, `hf-ops.ts`, `notes.ts` (parts), `page-background.ts`, `watermark.ts`, fonts `ooxml.ts` embedding | 4,180 | **split**: Opus for `source.ts` store/signatures (U6), `lossless.ts`, `WriteCtx`, the dynamic regexes (U7), fonts `ooxml.ts`; **Sonnet** for the element emitters inside them | D1a | 2 |
| **D2a** | editing session core (`session`, `history`) | session state, edit contexts, undo/redo, dirty, install/save orchestration, the settle step (U1), guarded and tracked text ops (`model-ops.ts`, `revisions.ts`, `edit-guard.ts`), clipboard model, formatting policy, the `Command` skeleton, the per-op identity rule (U3), the two-phase protocol (U9), the event table (U16) | ≈ 7,000 | **Opus** | D1a, D1b | 2 |
| **D2b** | edit-operation families (`session::ops::*`) | tables, sections/page, styles + numbering stores, notes, revision commands, drawing objects, headings/navigation, text layout options, paste rules, find; **after uo-docx-layout gate:** references/TOC/fields/captions/galleries/hyphenation (U1) | ≈ 14,300 | **Sonnet** frontiers; **Opus** find matcher and paste-tree boundary | D2a, uo-text (G10), `LayoutOracle` for the layout family | 2 |
| **D3** | Editing API (`api`, feature `editing-api`) | C# `Editing/**`, `CustomInlineTag.cs` | 4,750 (C#) | DOM + strict reading + serializer + host-callback design **Opus**; API translation **Sonnet** with the .NET-semantics table (U13); sanitizer reviewed by security (Opus) | uo-opc (strict mode, D6 resolution) | separate (.NET), G27 |
| **D4** | `.doc` conversion (`doc`, feature `doc`) | `doc/**` minus `cfb.ts`, `xmlw.ts` (uo-opc), `preset-map.ts` (uo-drawingml msospt), `officeart.ts` (shared, G11) | 5,090 (+ 580 shared) | **Sonnet**; the `odraw` API shape across the three copies is a short Opus note | uo-opc (cfb, flate, xml write), uo-drawingml (`msospt`, `odraw`) | separate |
| **D5** | derived documents (`export`, `odf`, `compare`; features) | `export/**`, `odf/**` (minus `zip.ts`), `compare/**` minus the shell element | 3,320 | **Sonnet** | D1a (D1b for ODF → docx round-trip tests) | 2 (export may join milestone 1 for the server) |

Total ≈ 46.0k lines of behaviour source (≈ 41.3k TS + 4.75k C#).

### Crate placement and reuse

- **uo-docx** (Write-only): everything above except the shared pieces below. Features:
  `session` (editor), `editing-api` (.NET hosts), `doc`, `odf`, `export`, `compare`. The browser
  editor build enables `session, doc, odf, export, compare`; the .NET package
  `editing-api, doc, export` plus the read path for PDF.
- **Shared, new or extended**:
  - *(revised after the skeptic, U11)* **`uo_docx::history`, a module, not a crate yet**: Write's
    undo semantics (state before the step; undo pushes the current state; partial states; dirty
    point; never clean after an undo) and the private `PVec<T>`. The three apps' semantics differ
    (Slides: before/after pairs, clean point, limit 100; Sheet: before/after snaps); a crate
    `uo-history` is created when Slides or Sheet port and the semantics converge (crate rule).
  - `uo-text` (new, G10): Windows word-sort collation (`sheet/packages/calc/src/collation.ts`, 182
    lines, already used by Write's table sort through `@universal-office/collation`), and Unicode
    property tables pinned to Node 24.15's V8/ICU (`\p{L}`, `\p{N}`, `\p{P}`, simple case folding
    for `/iu`) for find/replace and word boundaries. Sheet (SORT, MATCH, text functions) and
    Slides (find) reuse both. JS case mapping stays in `uo_jsnum::text` (uo-fonts S15). Needed by
    D2b only (milestone 2).
  - `uo-drawingml` additions (G11, G12): the colour resolver (`color::resolve(&DmlColor,
    &ThemeColors, &ClrMap, Variant)`, with a reader `color::read(&Element) -> DmlColor`) and the
    theme part reader (`a:theme` clrScheme/fontScheme/fmtScheme — the same part in docx, pptx, xlsx;
    today `ooxml/parts.ts` `readTheme`, `slides-core/theme.ts`, `sheet-core/theme.ts`), both in
    milestone 1; module `odraw` (feature `legacy`): the [MS-ODRAW] record reader, FOPT, shape tree,
    BStore/BSE/BLIP decode — three copies today (`docx-core/doc/officeart.ts` 304,
    `slides-core/ppt/officeart.ts` 376, `sheet-core/xls/officeart.ts` 400); `msospt` (uo-drawingml
    G6). The Word-only `contour.ts`/`shape-styles.ts` come **here** (G3 there).
- **Existing crates used as they are**: uo-opc (`Package` lazy, `xml_part` with the `docx-pos`
  preset, `resolve_target` (D6 only), `rels`, `ContentTypes`, `xml::raw` surgery
  (`split_element`, `set_kid`, `join_kids`, `with_attrs`, `insert_child`, `expand_self_closing`),
  `XmlWriter` and escapes, strict mode for D3, `cfb` and `flate` for D4); uo-jsnum A (`to_number`,
  `parse_int`, `round`, `conv`, the fdlibm trig used by `drawing.ts`) and **B** (`parse_float`, `Js`
  Display, `to_fixed` — the writers print numbers with `String(n)`, so B gates D1a/D1b);
  uo-fonts (`odttf::deobfuscate` for embedded fonts, `embed` plan two-phase for save-with-fonts,
  `face_info`); uo-testkit; uo-alloc.
- **Duplicates removed** (list in "Reuse").

### Data model, ownership, public API

*(revised after the skeptic, U2–U4, U10, U12, U17, U19)* The D1a model and the D2a session model
are designed together in **"Architecture (milestone 1)"** below: storage, identity ids, the flat
editing representation, UTF-16 text, the cleared mask, the undo snapshot shape, the layout-facing
API, the reader's output contract and the canonical digest. What follows here is the rest of the
crate (D1b, D3, D4, D5) and the session's outer API.

**D1b — writer**: no stored state. The TS module-level state of `serialize.ts` (`mediaOut`,
`drawingId`, `active`, `shapeTheme`, `chainNext`, `vmlSpid`, `vmlOnly`, `emittedShapes`, `freshId`),
`rev-xml.ts` (`nextId`, `usedUtc`) and *(revised after the skeptic, U17)* `picture-xml.ts`
`rawSrc` becomes one `WriteCtx` per `write_document` call; `emittedShapes`/`chainNext`/the per-save
`memo` are keyed by `DrawingId` (identity, U3), the source lookup of a textbox by its `SourceKey`
(so the `w:sdt`/`w:customXml` wrappers survive undo — U4c, fixed by name). Pristine signatures
(`runSig`/`propsSig`/`tableSig`) are computed over an order-insensitive canonical encoding (G23,
U6). Media dedupe by full byte equality (G24, U8). The writer borrows `&Document` and `&mut Package`.

**D3 — Editing API**: a mutable DOM `api::dom::Dom` (arena of nodes, interned `(namespace, local)`
names, free list with generations; `NodeId { index: u32, gen: u32 }` — a stale id is an error, never
UB), built from the uo-opc strict parser (namespaces resolved, DTD prohibited, depth 256 as the
C# `Parse`); `DocxDocument { pkg: Package, parts: lazily parsed Dom per XML part with dirty flags,
structure_changed, limits: DocxLimits }`. Views (`Shape`, `CustomInline`, `TableRowTemplate`,
`BodyWriter`) borrow the document or hold `NodeId`s. Save re-serializes dirty parts with a port of
the C# `Serialize` (XmlWriter rules: no BOM, no indent, `NewLineHandling.Entitize`, declaration
`encoding="utf-8"`, `CheckCharacters`, namespace declarations ensured on the root in the C#
`HashSet<XNamespace>` order) so saved parts are byte-equal to C# (G13). *(revised after the skeptic,
U13)* Host callbacks (`IsCustomInlineValid`, `UnwrapInvalid`, `ImportBody`'s section mapper) become
two-phase APIs (list `(type, json)` → host verdicts → apply); the `Regex`-taking APIs are dropped from
the facade (unused by clickCompliance; CHANGELOG); the brief carries a .NET-semantics table
(`OrdinalIgnoreCase`, `int.TryParse` styles, the .NET regex sites, `System.Text.Json` in
`CustomInlineTag`), and D3 regexes are differential against .NET 10, not JS.

**D4 — `.doc`**: `DocReader` (FIB, piece table text, FKP CHPX/PAPX, STSH, LST/LFO, SEP, PLCs for
notes/fields/bookmarks/comments, DOP, OfficeArt) as borrowed views over the CFB streams
(`uo_opc::cfb`), properties as small structs (`Chp`, `Pap`, `Tap`, `Sep`) with the "marked" bits of
`doc/props.ts`; output is docx bytes (`convert_doc_to_docx`), then the normal reader. Every offset in
`u64`, every count against the stream length, a work budget per input (uo-fonts pattern). Media keyed
by full bytes (G24), not `ext:length:FNV` of samples.

**D5 — export/ODF/compare**: `ExportDoc` borrows `&Document`; ODF reader produces a `Document`
(through uo-opc XML), writer consumes `&Document`; `compare` produces `DocDiff` over two
`&Document`s with a cap on edit distance work; the compare element queries documents through the
binding.

```rust
// uo_docx — outer API (session internals in "Architecture (milestone 1)")
pub struct DocxLimits { pub package: uo_opc::Limits, pub max_nesting: u32 /* 64 */, pub max_field_depth: u32, … }
pub enum DocxError { Package(uo_opc::Error), NotWordprocessing, Limit{..}, Corrupt{..} }
pub fn is_word_binary(bytes: &[u8]) -> bool;

pub mod read  { pub fn read_document(pkg: &Package, o: &ReadOptions) -> Result<Document, DocxError>;
                pub fn read_embedded_fonts(pkg: &Package) -> Vec<EmbeddedFont>; }
pub mod write { pub fn write_document(pkg: &mut Package, doc: &Document, o: &WriteOptions) -> Result<(), DocxError>;
                pub fn write_font_embedding(pkg: &mut Package, plan: &uo_fonts::embed::Plan, s: &EmbeddingSettings); }
pub mod session {
    impl Session {
        pub fn open(bytes: Vec<u8>, o: OpenOptions, env: Box<dyn Env>) -> Result<Session, OpenError>; // docx, .doc, .odt
        pub fn new_blank(o: BlankOptions, env: Box<dyn Env>) -> Session;
        pub fn exec(&mut self, cmd: Command, cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError>;
        pub fn complete(&mut self, job: JobId, result: JobResult, cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError>; // two-phase (G25)
        pub fn query(&self, q: Query) -> Answer;
        pub fn undo(&mut self, cx: &mut ExecCx<'_>) -> ChangeSet; pub fn redo(&mut self, cx: &mut ExecCx<'_>) -> ChangeSet;
        pub fn save_docx(&mut self) -> Result<Vec<u8>, SaveError>;
        pub fn save_docx_with_fonts(&mut self, faces: &[uo_fonts::embed::Face]) -> Result<Vec<u8>, SaveError>;
        pub fn export(&self, f: ExportFormat) -> Result<Vec<u8>, ExportError>; // odt, html, markdown
        pub fn view(&self) -> DocView<'_>;          // what uo-docx-layout reads
        pub fn edits_since_load(&self) -> u32;      // PDF from the original bytes when 0 (TS rule)
        pub fn clear_caches(&mut self);
    }
    pub struct ExecCx<'a> { pub layout: &'a mut dyn LayoutOracle, pub clipboard: &'a mut ClipboardRing }
    pub enum Outcome { Done(ChangeSet), NeedsJobs(Vec<Job>) }  // Job = decode/encode/clipboard read for the shell (G25)
}
pub mod api { pub struct DocxDocument; … }                 // feature editing-api (D3)
pub mod doc { pub fn convert_doc_to_docx(b: &[u8], l: &DocxLimits) -> Result<Vec<u8>, DocError>; }
pub mod odf { pub fn read_odt(b: &[u8], l: &DocxLimits) -> Result<Document, DocxError>; pub fn write_odt(d: &Document, o: &OdtOptions) -> Vec<u8>; }
pub mod export { pub fn to_html(d: &ExportDoc, o: &HtmlOptions) -> String; pub fn to_markdown(d: &ExportDoc, o: &MarkdownOptions) -> String;
                 pub fn docx_to_html(b: &[u8], o) -> Result<String, DocxError>; pub fn docx_to_markdown(b: &[u8], o) -> Result<String, DocxError>; }
pub mod compare { pub fn diff_documents(a: &Document, b: &Document, o: &DocDiffOptions) -> Result<DocDiff, CompareError>; }
```

`Command` is an enum of family enums (`Text`, `Format`, `Paragraph`, `List`, `Style`, `Table`,
`Section`, `HeaderFooter`, `Note`, `Reference`, `Revision`, `Comment`, `Find`, `Drawing`,
`Picture`, `Page`, `Clipboard`, `Context`, …), one variant per model-changing public method of
`UniversalWriteElement` (≈ 300 of the 532) plus *(revised after the skeptic, U16)* the context
entries the TS reaches through private methods (`enterCell`, `enterNote`, `enterTextbox`); D2a owns
the enum and `Session`, each D2b frontier one `impl Session` module. `ChangeSet { stories:
Vec<StoryChange>, selection, events: Vec<Event>, dirty, can_undo, can_redo }`.

**Changed from 0.9 for the better** (CHANGELOG at the switch): one session object instead of model
state scattered over the element; clock/RNG/locale injected (deterministic tests); the C#
`UniversalOffice.Docx` public API replaced by the facade over `api` (names kept where clickCompliance
uses them: `DocxDocument.Open/Save`, `DocxSanitizer.Sanitize`, `DocxTemplate.*`, `DocxFields.*`,
`CustomInlineTag`, `DocxBodyWriter`; host callbacks two-phase); the C# reader/model/export classes are
no longer public; standalone docx → Markdown/HTML goes through the model (G14).

### The interface the TS shell needs (wasm boundary, M4)

Not the binding design (that is the `uo-wasm` plan); what it must carry:

- **One handle per open document**: `UoWriteSession` (owns `uo_docx::Session` and, in the binding
  crate, the uo-docx-layout state, which also implements `LayoutOracle` — so uo-docx does not depend
  on layout). `free()` plus a `FinalizationRegistry` path, as the foundation's `UoDocument`. One
  `ClipboardRing` per wasm instance (G7). No other long-lived handles: views and results are values.
- **Hot paths as dedicated exports** (called per keystroke/pointer move): insert typed text, delete
  backward/forward, split paragraph, set selection, move caret (with layout), hit-test and caret
  rectangles (layout), returning a compact `ChangeSet`. *(revised after the skeptic, U16)* The
  cancelable `uw-before-input` stays in the shell, before the call.
- **Everything else through `exec(handle, cmd)` / `query(handle, q)`** with a schema-generated
  binary codec (one generator emits the Rust enums and the TS encoder/decoder; the same generator
  serves the uo-render display list — G15). Queries cover the toolbar state (`getSelectionState`,
  list info, styles in use, table state, picture/shape format), revisions/comments/bookmarks/
  headings/notes lists, find hits, ruler info, paragraph texts and languages for the spellchecker,
  and the documents for the compare element.
- **Events** *(revised after the skeptic, U16)*: the session emits the model-derived `uw-*` events
  (`change`, `cellmode`, `notemode`, `hfmode`, `tbmode`, `protected`, `font-embedding`,
  `revisions-changed`, `comments-changed`, `bookmarks-change`, `styles-change`, `dirty-change`,
  `caption-settings`, `track-changes`, `sort-error`, `pasted`, `custom-inline-inserted`, `blocked`,
  `theme-change`; the full ≈ 38-type table with which are shell-only is in the D2a note); the shell
  re-dispatches them in order.
- **Two-phase jobs** *(revised after the skeptic, U9, G25)*: commands that need browser codecs or the
  system clipboard (`compressImages`, `changeImage*`, `setShapePictureFill`, `insertImage`,
  `setImageAdjust`, `renderClipPng`, `paste`) return `Outcome::NeedsJobs`; the shell runs them and
  calls `complete`. Parity feeds the shell's bytes to both sides; re-encoded bytes are not a parity
  output.
- **Bytes**: open (JS writes into `uo_alloc_bytes`, ownership moves in), save/export (Rust-owned
  buffer, JS copies and frees), images in, clipboard fragments out/in (an opaque versioned
  fragment format with the session id + generation instead of the TS document identity/`WeakRef`
  (U3), plain text, HTML from `export`).
- **Fonts on save**: two phases, no callbacks: font usage (layout) → the shell gathers the face bytes
  it holds → `save_docx_with_fonts`.
- **Paste HTML** *(revised after the skeptic, U16)*: the shell keeps `DOMParser` and sends a neutral
  paste tree: per element the tag, the **inline CSSOM longhands** it reads today
  (`el.style.fontWeight`…, colours as `rgb()`), the raw `style` attribute (mso-list), comment nodes,
  text, list and image refs; depth- and size-limited. Chromium's CSSOM is the reference. The rules of
  `paste-html.ts` and the policy sanitization run in Rust.

### .NET (D3 and the PDF path)

C# facade in `dotnet/`: `DocxDocument : SafeHandle`; view types are `readonly struct`s of
(document handle, `NodeId`), so no extra native handles exist; byte buffers are returned owned and
copied; host verdicts passed as arrays (two-phase, U13). M4: 10,000 documents opened/disposed and
finalized; LeakSanitizer on the Linux CI. Whether D3 ships before the Rust PDF path (a NuGet with the
Rust Editing API next to the C# reader/layout/PDF) is G27.

### Crate vs own port

| Need | Candidates | Recommendation |
|---|---|---|
| XML, zip, rels, raw surgery | uo-opc | use (done) |
| Mutable DOM for D3 | `roxmltree` (read-only), `xmltree`/`minidom` (own parsers: DTD and entity behaviour differ from the strict mode; `minidom` MPL-2.0) | **own** `api::dom` on the uo-opc strict parser |
| Regex for XML surgery *(revised after the skeptic, U7: parser count ≈ 550–740 sites — docx-core 350 literals + 34 `new RegExp`, editor model files 156 + 13, universal-write.ts 181 + 7; ≈ 55 built at run time from document data; 14 with `u`)* | `regex` (linear; `\s`, `\b`, `i`, lookaround differ from JS; ≈ 0.3–0.6 MB wasm), `regress` (ECMAScript semantics, backtracking — ReDoS on untrusted XML) | **own hand matchers** per site from a spec table, differential-fuzzed against JS `RegExp`; shared templates for the dynamic sites (element by tag/attribute with the value matched as a literal; the TS metacharacter/`SyntaxError` behaviour accepted by name); `fieldCodeXml` as a bounded lazy-group matcher with a work budget (G9) |
| Find/replace engine | `regress`, `fancy-regex` | **own matcher** over the Word query AST, Unicode tables from `uo-text`, step budget (Opus) |
| Persistent vectors for undo | `im`/`imbl`/`rpds` (MPL-2.0) | **own** `PVec` in `uo_docx::history` |
| Ordered maps (TS `Map` order) | `indexmap` (MIT/Apache, already allowed in uo-opc) | use |
| Collation | `icu_collator` (ICU order ≠ Windows/Excel word sort) | **own port** of `collation.ts` in `uo-text` |
| Unicode properties | `unicode-general-category`, `icu_properties` (versions not pinned to V8's) | **generated tables** pinned to Node 24.15 |
| HTML parsing for paste | `html5ever` (≈ 0.5 MB wasm; not the CSSOM the TS reads) | **none**: the shell's `DOMParser` + CSSOM (G15) |
| Diff (compare) | `similar` (different tie-breaking) | **own port** |
| Date formatting | `chrono`, `icu_datetime` | **own**: `Env::now()` gives local date parts; Word date pictures belong to `datefmt` (uo-docx-layout) |
| Small vectors (`BlockPath`) | `smallvec` (MIT/Apache) | optional; a fixed `[u32; 8]` + overflow `Vec` is enough |

`#![forbid(unsafe_code)]` in uo-docx.

### Parity

*(revised after the skeptic, U5, U6, U8, U15, U16)* Common infrastructure, built alongside the port
(hybrid mode):

- **Canonical model stream**: specified in "Architecture (milestone 1)" (TS walker + Rust `Canon`).
- **Where captures run**: D1a, D1b, D4 and D5 capture in **Node 24.15 x64** (uo-jsnum G2: the math
  reference). The D2 editor harness runs in Chromium (Playwright) with `Math.sin/cos/tan/atan2/pow/
  hypot/exp/log` replaced by the uo-jsnum wasm exports through `addInitScript` (recorded in the
  capture metadata), one element per page, checkpoints taken after the queued refresh, the idle
  relayout, hyphenation and font loads have settled, and TS-`private` entry points called on an
  unminified bundle.
- **Determinism**: `Math.random` replaced by the testkit xorshift32 seeded per item, `Date` fixed;
  Rust uses an `Env` with the same generator and clock (G8).
- **Key order** (U6): the TS signatures and `sameJson` comparisons are order-sensitive; Rust's are
  not. The harness counts the steps where the order-sensitive and order-insensitive verdicts differ
  **and** the output bytes differ; 0 on the corpus → accepted by name (G23); otherwise an order tag on
  the affected objects.

| Sub-module | Capture points | Corpus | Compared |
|---|---|---|---|
| D1a | `readFullDocument(pkg, {sources: true})`, `readDocFormatting`, `readEmbeddedFonts`, `readStyleSheet`, `inspect` queries, per-frontier facets (props, parts, drawing, paragraph, full) | 2,742 oracle docx, docx-core fixtures, the uo-opc quirk corpus, a **generated attribute-quirk corpus** (numbers as `"1e3"`, `" 12 "`, `"12pt"`, `"0x10"`, `"-0"`, `"NaN"`, empty, missing; integer-like keys; duplicate ids; deep nesting past 64; `basedOn` cycles and 10k chains; non-BMP text) | canonical model stream, exact |
| D1b | `writeDocument` after install with no edit; after every D2 script checkpoint; `serializeBlocks` on generated models; fonts `writeDocxEmbedding` | oracle + scripts + a media quirk pair (same length, bytes differing off the 509-byte samples — U8) | every part's decompressed bytes exact; re-read → model stream equal |
| D2a/D2b | the Chromium harness: public API, synthetic input events, private context entries; at each checkpoint `stringifyState(true)` canonicalized + selection + `saveDocx()` + events | scripts from the lab oracles, the test suites, generated sequences; non-BMP scripts included (G6) | state stream after every step and every undo/redo, saved bytes, events |
| D3 | the C# suite ported to Rust tests; C# API over the oracle corpus | oracle + `dotnet/tests` fixtures + hostile corpus | dirty parts byte-exact to C#, results exact, exceptions by class |
| D4 | `convertDocToDocx` + `DocReader` facets | 11 repo `.doc` (CI); **Word-generated `.doc` from the oracle docx** (`doc-lab/gen-fixtures.ps1`, `SaveAs2` format 0) and the local third-party corpus — generators and hashes committed, files local (G18); CFB/FIB/FKP/sprm mutations | docx parts exact; error class |
| D5 | `toHtml`/`toMarkdown(exportDocFromFullDocument(…))`, `writeOdt`, `readOdt` on **Word-generated (`SaveAs2` 23) and LibreOffice-generated `.odt`** and on `writeOdt` output, `diffDocuments` pairs | oracle + scripts + generated `.odt` | strings and parts exact |

The C# ports (reader, DocBinary, Export) are not parity references (TS is); differences found
between them are reported, not fixed (C# is frozen).

### Memory (measured gate)

| Sub-module | M1 (no leak per op, incl. error paths) | M2 soak (tolerance) | M3 budgets (first record; references) | M5 caches and caps |
|---|---|---|---|---|
| D1a | read, every reader error (limit hit mid-part, corrupt) | 1,000 × (open → read → drop) over 20 corpus docs incl. the largest oracle; 0 B; wasm `byteLength` flat | model bytes vs TS heap delta of `readFullDocument` (node `--expose-gc`), per doc class; peak ≤ 1.5× the XML text read; UTF-16 text cost reported separately | style resolution cache (per document; 100k generated styles) |
| D1b | save untouched, save after edits, writer errors | 1,000 × (open → read → save → reopen → drop); 0 B | allocated ≤ 3× the XML bytes written | none |
| D2a | every `Command` family, undo, redo, jobs, error paths | 1,000 × (open → 100 scripted edits incl. tables, notes, header/footer, tracked changes → 50 undo → 50 redo → save → close); 0 B after `clear_caches`; wasm flat | **per keystroke on a generated 500-page document with ≈ 2k bookmarks, many tables and sections (U19): allocated ≤ 64 KiB**; step cost = bytes not shared with the next state; 200-step history ≤ 2× model bytes | history: 200 steps **and** a byte cap (64 MiB wasm32 / 256 MiB native) counted without double-counting shared chunks; *(revised after the skeptic, U12)* the style/numbering version stores collected with the history and inside its cap (10,000 list toggles); `ClipboardRing` 24 clips + byte cap |
| D2b | per family, with the D2a harness | in the D2a soak | per family where heavy (sort, find all, TOC update on the largest doc) | find: none stored beyond the last query |
| D3 | every API call | 1,000 × (`Open` → `Sanitize(Content)` → `ApplyContent(template)` → `Save`); 0 B | vs C# `GC.GetAllocatedBytesForCurrentThread` (reference only) | parsed DOM per part (dropped on save/close) |
| D4 | convert each fixture, every error | 1,000 × convert the fixtures; 0 B | allocated ≤ k × input, worst case per limit | none |
| D5 | export, ODF read/write, diff | 1,000 × (read → html → markdown → odt → read_odt → diff) | diff work capped | none |

M4: `UoWriteSession` (10,000 create/free and finalizer path; with layout state inside), owned
result buffers, `ClipboardRing`; .NET `DocxDocument` SafeHandle. M6: Miri only if `unsafe` appears
(none planned); LeakSanitizer for the FFI path.

### Security

Untrusted inputs: docx/docm/dotx (browser and server), `.doc`, `.odt`, pasted HTML trees,
clipboard fragments, find patterns, table formulas, field instructions/codes, custom-inline JSON in
`w:tag`, images.

- Limits: `uo_opc::Limits` (depth 1,000 for `docx-pos`, 256 strict for D3), `DocxLimits` (C# fields
  plus `max_nesting` 64, `max_field_depth`, runs/paragraphs per part, numbering levels 9, `basedOn`
  resolved iteratively with a seen set), D4 work budget, paste tree depth and size, find step
  budget, formula parser depth, `fieldCodeXml` matcher budget (U7), diff work cap.
- *(revised after the skeptic, U18)* **Outcome past `max_nesting`**: content deeper than 64 levels
  (tables in cells, textboxes, groups) is read as plain text paragraphs in place (accepted by name),
  the file is not rejected; one wasm test at the limit runs read → write → layout → digest with the
  configured stack size.
- The **sanitizer** (D3) is the security boundary of clickCompliance: parity with C# plus the security
  review; the host's `DocxPackageValidationService` keeps resolving targets with uo-opc D6.
- Exported HTML of an untrusted document: escaping and link schemes as the TS does them; anything
  unsafe listed for the maintainer.
- Fuzz targets: `read_docx` (small limits), `read_part_{styles,numbering,theme,settings,document,notes}`,
  `roundtrip`, `lossless_surgery`, `edit_script` (no panic, positions valid, undo restores the state
  stream exactly, save → reopen succeeds), `paste_tree`, `find_query`, `table_formula`,
  `field_code_xml`, `api_open_sanitize_save`, `api_template_apply`, `field_instruction`,
  `custom_inline_tag`, `doc_convert`, `odraw_records`, `odt_read`, `diff`.

### JS semantics and risks (each becomes a facet or an accepted difference)

1. *(revised after the skeptic, U2)* **UTF-16 text and positions.** Backspace and caret-left move one
   code unit (universal-write.ts:9809–9812, 9964), so the TS model holds lone surrogates after
   editing non-BMP text; the save writes them as U+FFFD (`TextEncoder`). The model stores UTF-16
   (G6), so this is reproduced, not snapped.
2. **`JSON.stringify` as equality, signature and undo**: key order (U6), dropped `undefined` keys
   and symbols, `NaN`→`null`, `-0`→`0`, lost identity (U3, U4) — see the architecture.
3. **Object identity**: `reconcileBlocks` by paragraph identity, the three aliased views of a table
   cell re-aliased by `TOps.relink` after undo, `SourceStore.txbx` (`WeakMap` by textbox),
   `emittedShapes`/`memo` per save, `RichClip.doc`/`WeakRef`, `a.rpr === b.rpr` fast paths,
   `secs.find((s) => s.sectPr === sb.sectPr)`, the image pool, `floatSel` (U3): explicit ids.
4. **Iteration order**: `Map`/`Set` insertion order; integer-like keys of plain objects ascending.
5. **Numbers**: `parseInt`/`Number()`/`parseFloat` (≈ 170 sites in docx-core), `Math.round`
   (≈ 150), trig in `drawing.ts`, `String(n)`/`toFixed` in writers — all through uo-jsnum A/B.
6. **Regex** (≈ 550–740 sites, ≈ 55 dynamic, U7).
7. **Locale and time**: `toLocaleDateString('pt-BR')`, `toLocaleUpperCase('pt-BR')`,
   `Intl.Collator('pt-BR')`, `Math.random` ids, `Date.now()` → `Env`.
8. **Process and instance state** (G7, U17): `wrapIds`, `blockSdtKeys`, `keySeq` never reset;
   `nextBmId` not reset on load; `nextSectionId`, `docToken`; per-save counters; the cross-instance
   `OFFICE_CLIPBOARD`.
9. **Exceptions as control flow** (`try {…} catch {}` with partial results): explicit `Result` paths.
10. **Recursion depth**: iterative or limited (U18).
11. **Sort**: stable on both sides; inconsistent comparators captured.
12. **The TS flat editing representation** (G3) kept, including the cell views (U3).
13. *(revised after the skeptic, U1)* **The layout pass mutates the model** (`syncSections`,
    `syncRunRPr`, field results): the settle step and the `LayoutOracle`.

### Order of work (hybrid mode)

*(revised after the skeptic and the hybrid mode)*

```
milestone 1 (read path):  uo-jsnum B ┐  uo-drawingml (+ resolver, theme) ┐
                                     ├── D1a model+reader (R1–R6, Sonnet, inside the architecture)
                                     │        └── uo-docx-layout (Opus; editor-facing API and LayoutOracle specified in its plan)
                                     │               └── uo-render ── uo-pdf ──► viewer + server PDF  [one maintainer gate]
milestone 2 (editor):     D1b ── D2a (Opus) ── D2b frontiers (Sonnet; layout family after uo-docx-layout) ── D5 ── bindings ──► editor switch [one gate]
separate:                 D3 Editing API (now; ships per G27)      D4 .doc (+ odraw, msospt)
```

The browser editor switches at once (fixed decision). Milestone 1 is internal or a preview; 0.10 is
released at the editor switch (U10; maintainer).

### Models (closed vs open scope)

*(revised after the skeptic)* As the skeptic's "Model verdict per sub-module", adopted as is. In
short: Opus for the architecture (done below), D1b core, D2a, the find matcher and paste boundary,
the D3 DOM/serializer/callbacks and the `odraw` shape; Sonnet for the D1a reader (R1–R6), the D1b
emitters, D2b, the D3 API translation, D4 and D5. The orchestrator moves a part to Opus after
repeated parity rounds on the same issue.

## Architecture (milestone 1)

*(new, after the skeptic and the hybrid mode)* The big shapes for the read path, designed together
with the editing session so that milestone 2 does not reshape what uo-docx-layout already reads (U10).
Everything here is a decision for the milestone-1 gate (list at the end of the record).

### A1. Principles

1. **One document model** for the viewer, the PDF path and the editor. The TS has three overlapping
   shapes (the `FullDocument` blocks, the editor's flat paragraphs + table anchors, and the aliased
   views inside cells, header/footer sets and notes); Rust stores each story **once**, in the editor's
   flat shape (G3), and derives the block order.
2. **A tree with structural sharing, not a graph with interior mutability**: nodes are immutable
   behind `Arc` and changed by copy-on-write (`Arc::make_mut`); a snapshot is a clone of the roots.
   Cross references (textbox chains, notes, comments, bookmarks, header/footer parts, media) are
   typed ids, never pointers. No `Rc<RefCell>`, no `Weak`.
3. **Identity is a value** (U3, G22): what the TS decides by `===` (paragraph, table, drawing object)
   is an explicit id; `Arc::ptr_eq` is only a "content unchanged" fast path (layout caches), never
   identity — copy-on-write changes the pointer where the TS object keeps its identity.
4. **Text is UTF-16** in the model (U2, G6): `Box<[u16]>` per run; positions are code units;
   UTF-8 exists only at the XML boundaries (reader decodes, writer encodes lone surrogates as U+FFFD
   like `TextEncoder`). uo-docx-layout indexes UTF-16 too and passes code units (lone surrogates
   included, as the TS `codePointAt`) to uo-fonts.
5. **The TS-visible state is the parity contract**: every field the TS can observe (including an
   explicit `undefined` key, a stored `NaN`, a `null` after undo) has a representation.

### A2. Storage

```rust
// ── ids (per document; allocated by IdAlloc; Copy, 4 bytes) ───────────────────────────────
pub struct ParaId(u32);  pub struct TableId(u32);  pub struct DrawingId(u32); // textbox/shape/group/image object
pub struct HfPartId(u16); pub struct MediaId(u32); pub struct SourceKey(u32);   // SourceKey = TS `src`, a value
pub struct SectionBreakId(u32);                                                // TS sectionBreak.id (nextSectionId)

pub struct Document {
    pub body: Story,
    pub sections: Arc<[Section]>,            // reader output: sectPr, SectionProps, block range, HfPartId per slot
    pub hf: PVec<HfPart>,                    // HfPartId: path, rid, kind, story
    pub notes: Arc<Notes>,                   // footnotes/endnotes: IndexMap<i32, Story> (TS Map order) + special ids
    pub comments: Arc<[Comment]>,
    pub bookmarks: Arc<Bookmarks>,           // IndexMap<name, BookmarkPos { block, offset, end }> (TS Map order)
    pub formatting: Arc<DocFormatting>,      // resolved styles, numbering, theme, settings, font table, raw part texts
    pub core_props: Option<CoreProps>, pub background: Option<Arc<PageBackground>>,
    pub media: MediaStore,                   // MediaId -> Arc<[u8]> + ext; dedupe by full bytes (G24)
    pub sources: Option<Arc<SourceStore>>,   // immutable after read
    pub ids: IdAlloc,
}

/// One story, in the editor's flat shape (G3): paragraphs + tables anchored "before paragraph N".
/// The body, every table cell, every header/footer part, every note and every textbox is a Story.
pub struct Story {
    pub paras: PVec<Paragraph>,              // persistent chunked vector of Arc<Paragraph>
    pub tables: PVec<TableAnchor>,           // { before_para: u32, table: Arc<Table> }, in TS anchor order
}
pub struct Paragraph { pub id: ParaId, pub runs: Vec<Run>, pub props: Arc<ParaProps>,
                       pub ppr: Option<Arc<PPr>>, pub ppr_edited: bool }
pub struct Run {
    pub text: Utf16,                         // Box<[u16]>
    pub props: Arc<RunProps>,                // incl. `rpr_link`: the TS symbol keys RPR_REF/RPR_EDITED live on the props
                                             // object, so the spread copies them; invisible to equality and the digest,
                                             // dropped by normalisation
    pub rpr: Option<Arc<RPr>>, pub rpr_edited: bool,
    pub ext: Option<Box<RunExt>>,            // rare fields: image, textbox, shape, group (each with a DrawingId), note_ref,
                                             // field, field_code, field_result: Vec<Run>, custom, ptab, sym_font, in_toc,
                                             // form_field, math, bookmark, field_begin, field_end, field_open
}
pub struct Table { pub id: TableId, pub rows: Vec<Row>, pub grid_cols: Vec<i32>, pub borders: Option<TableBorders>,
                   pub tbl_pr: Option<Arc<TblPr>>, pub src: Option<SourceKey> }
pub struct Cell { pub content: Story, pub tc_pr: Option<Arc<TcPr>>, pub width: Option<i32>, pub shading: …,
                  pub grid_span: u32, pub borders: …, pub src: Option<SourceKey>, pub view: CellView }
```

- **Cells** (U3): the TS `paragraphs`/`tables`/`blocks` triple is one `Story`; `CellView` records
  which TS view is current, because the TS keeps `blocks` stale while a cell is edited through
  `paragraphs` and merges them back by `reconcileBlocks` (identity placement). The reader always
  produces `CellView::Synced`; D2a implements the stale/reconcile rule on top (placement by `ParaId`).
  The same for header/footer sets (`headers.default` vs `headerBlocks.default`), notes
  (`footnotes` vs `footnoteBlocks`) and textboxes (`paras` vs `blocks`).
- **Block order** is derived: `Story::blocks()` yields `Block::{Paragraph(&Paragraph), Table(&Table)}`
  by merging the anchors (TS `buildEngineDoc` order: anchors of paragraph N before it, in anchor
  order; anchors ≥ len at the end). `Section` block ranges and `BookmarkPos.block` are indices in
  that order, exactly as the TS reader gives them.
- **Props** (U4, the cleared mask): `RunProps`/`ParaProps` (and `SectPr`) are structs of `Option<T>`
  with the TS field names plus `cleared: FieldMask` (one bit per field: "the TS object has this key
  with the value `undefined`"). `Patch::Clear` sets the bit; one helper `spread_over(base, over)`
  reproduces `{ ...base, ...over }` (an `over` field present → it; cleared bit → `None` and the bit
  kept; else `base`); `Object.assign` is `assign_into`. Equality used by the TS through
  `JSON.stringify` ignores the mask; the canonical digest does not (`'U'` entries).
- **Stored `NaN`/`±∞`** (U4b): numeric fields the reader can fill from `parseFloat`/`Number` of a
  bad attribute are `JsNumber` (`Absent` in the `Option`, `Num(f64)` including NaN/±∞, and `Null`
  for the state after a JSON round trip, which compares as `!== undefined` and is 0 in arithmetic).
  Which fields need it is a declaration of the R1–R6 frontiers, checked by the quirk corpus; the rest
  stay `Option<f64>`/integers.
- **Colours**: `DmlColor { base: ColorBase /* Srgb(u32) | Scheme(slot) | Sys{name,last} | Prst(name) */,
  mods: Vec<ColorMod> }` (uo-drawingml, G12); the reader applies `parse_int` at read time (the TS
  string encoding `"RRGGBB|mods"` is not ported).
- **Sources** (`SourceStore`): `ParaSource { raw: Span, open: Span, at: Option<u32>, ppr: Option<Span>,
  segs: Vec<SegSource> }`, `RunSource`, `TableSource`, `WrapSeg`, `StorySource`, `NotesPartSource`;
  `Span { part: PartSrc(u16), start: u32, end: u32 }` (UTF-8 offsets into the part's
  `uo_opc::xml::Source`, `Arc<str>`). Keyed by `SourceKey` (the TS `src` numbers, values that survive
  undo); the textbox lookup is by `SourceKey` too, which fixes the TS loss of the textbox's
  `w:sdt`/`w:customXml` wrappers after an undo (U4c, accepted by name). The baseline signatures
  (`sig`) are not in the reader output: the session takes them at install (milestone 2), over the
  order-insensitive encoding (G23).
- **Formatting**: `DocFormatting { styles: StyleSheet (IndexMap by styleId, TS order), resolved:
  StylesMap (basedOn + docDefaults, resolved iteratively), numbering, theme: uo_drawingml::Theme,
  settings, font_table, parts: raw texts for the lossless stores }`. In the session the
  styles/numbering stores keep versions as `Arc<str>` collected with the history (U12, G26).
- **Generated data** (`word-styles.ts`, `style-names-es.ts`, `table-styles-builtin.ts`, `blank.ts`
  parts, later `list-library.ts` and galleries): checked-in tables + generator + regeneration diff test.
- **Nesting**: `max_nesting` 64 with the text fallback (U18, G20); the reader, digest and writer walk
  stories iteratively (explicit stack of `(story, cursor)`).

Memory scopes: **per document** — `Document` (the session owns it; the viewer/PDF path owns it
alone); **per history step** — `EditState` holding `Arc`/`PVec` roots (shared chunks); **per
operation** — temporaries; **per layout pass** — only in uo-docx-layout, keyed by ids.

### A3. The session model (milestone 2, fixed now so the types do not move)

```rust
pub struct Session {
    pkg: uo_opc::Package, doc: Document,
    ctx: EditContext,          // Body | Cell { anchor: u32, hops: CellHops } | Note { kind, id }
                               // | HeaderFooter { part: HfPartId, page: u32 } | Textbox { drawing: DrawingId, … }
    selection: Selection,      // anchor/focus: Pos { para: u32, offset: u32 } — UTF-16, in the context's story
    table_sel: Option<TableSel>, float_sel: Option<DrawingId>,
    styles: VersionStore, numbering: VersionStore,     // part texts by version, collected (U12)
    settings: SettingsEdits,   // even/odd, default tab, hyphenation, note settings, background, font embedding, tracking
    counters: SessionCounters, // next_section_id, next_bm_id (reset per document, U17), ref seed, …
    policy: Option<ResolvedPolicy>, author: Box<str>,
    history: History<EditState>, dirty: DirtyState, env: Box<dyn Env>,
}
/// One undo step = the TS `stringifyState` fields, as shared roots (U19: every collection persistent).
pub struct EditState {
    m: Story /* or the context story */, s: Selection, anc: Option<(PVec<TableAnchor>, Arc<Bookmarks>)>,
    sv: VersionId, nv: VersionId, dt: Option<i32>, hy: Option<Arc<Hyphenation>>,
    tb: Option<PVec<TableAnchor>>, ce: Option<CellCtx>, ts: Option<TableSel>,
    hf: Option<Arc<[[Option<Story>; 6]]>>, nt: Option<NoteCtx>, nb: Option<Arc<[(NoteKind, i32, Story)]>>,
    np: Option<Arc<NoteSettings>>, te: Option<TextboxCtx>, fs: Option<u32>,
    hp: Option<(HfKey, Story, Selection)>, hx: Option<Arc<[(HfKey, Story)]>>, eo: Option<bool>,
    bg: Option<Option<Arc<PageBackground>>>, bgd: Option<bool>,
}
```

- **Snapshot**: clone of the roots, O(touched chunks); `PVec` chunks of 64 `Arc<T>` shared between
  steps; step cost for the byte cap = bytes not shared with the next state (pointer comparison at
  push), so shared chunks are not double-counted (U19).
- **Restore** applies `EditState::normalize` once (U4: at restore, not at snapshot): drops the
  cleared masks and `rpr_link`s (symbols), maps stored `NaN`/`±∞` to `Null`, `-0` to `0`, and gives
  fresh `ParaId`/`TableId`/`DrawingId`s (the JSON copies are new objects); `SourceKey`s and media
  ids survive (values / the image pool). O(document) per undo, as in the TS.
- **Identity rule per op** (U3, G22): `story.para_mut(i)` keeps the id (TS in-place mutation);
  `story.replace_para(i, p)` / `insert_para` takes a new id (TS literal or `{ ...p }`); each D2 op
  states which, per TS construction site.
- **The settle step** (U1, G21): `Session::settle()` runs at the end of every `exec`/`undo`/`redo`
  where the TS `refresh()` runs: the layout-independent normalisations `syncSections` (section
  break ids from `next_section_id`, `p.props` replaced, header/footer refs migrated) and the rpr
  inheritance of `syncRunRPr`. Layout-dependent mutations go through the `LayoutOracle` (A4). The TS
  fallback "layout failed → legacy layout + `exitCell()`" is dropped (layout errors are values;
  accepted by name).

### A4. The layout-facing API

What uo-docx-layout reads and calls; enough for its plan to start.

```rust
/// Read-only view of what the layout needs; borrowed from a Document or a Session.
pub struct DocView<'a> { doc: &'a Document, context: Option<ContextView<'a>> /* the edited story, editor only */ }
impl<'a> DocView<'a> {
    pub fn body(&self) -> &'a Story;                   // Story::blocks() for the block order
    pub fn sections(&self) -> &'a [Section];           // block ranges, SectPr, SectionProps, hf slots
    pub fn hf_story(&self, part: HfPartId) -> &'a Story;
    pub fn notes(&self) -> &'a Notes; pub fn comments(&self) -> &'a [Comment];
    pub fn bookmarks(&self) -> &'a Bookmarks;          // name -> block/offset/end (REF, PAGEREF)
    pub fn formatting(&self) -> &'a DocFormatting;     // styles (resolved + raw), numbering, theme, settings, fonts
    pub fn core_props(&self) -> Option<&'a CoreProps>; // CREATEDATE/SAVEDATE/PRINTDATE
    pub fn background(&self) -> Option<&'a PageBackground>;
    pub fn media(&self, id: MediaId) -> Option<&'a [u8]>;
    pub fn revision_authors(&self) -> Vec<&'a str>;    // TS revisionAuthors order
    pub fn formatting_version(&self) -> u64;           // bumps on any style/numbering/settings/section change
}
/// References from layout output back to the model (TS "b12", "b3/r0/c1/b0").
pub struct DocRef { pub story: StoryRef, pub path: BlockPath, pub offset: u32 /* UTF-16 */ }
pub enum StoryRef { Body, HeaderFooter(HfPartId), Note(NoteKind, i32), Textbox(DrawingId), Context }

/// Implemented in the binding by uo-docx-layout; uo-docx never depends on layout.
pub trait LayoutOracle {
    /// Full layout with `onField`: the last value of each dynamic field run (TS refreshFieldCache).
    fn field_values(&mut self, doc: DocView<'_>, scope: FieldScope) -> Result<Vec<(RunRef, Utf16)>, LayoutError>;
    fn pages_of(&mut self, doc: DocView<'_>, at: &[DocRef]) -> Vec<Option<u32>>;   // TOC, PAGEREF, captions, go-to
    fn next_hyphenation(&mut self, doc: DocView<'_>, from: DocRef, skip: &HyphenSkips) -> Option<HyphenCandidate>;
    fn font_usage(&mut self, doc: DocView<'_>) -> Vec<FontUsage>;                    // save with fonts
}
```

- **Incremental relayout** (the TS `tryFastLayout`/`tryFastTable`): the layout caches per
  `ParaId`/`TableId` and checks `Arc::ptr_eq` of the node it laid out (unchanged pointer ⇒ unchanged
  content, free from copy-on-write); `ChangeSet.stories` lists `StoryChange { story, changed:
  Vec<ParaId>, structure: bool }` (insert/delete/split/merge set `structure`), and
  `formatting_version` invalidates everything. The idle full relayout after a fast one (1.5 s) and the
  "one paragraph, same line heights" rule stay layout/binding decisions.
- **Field values** are applied by the session (`setFieldResult`) inside `exec(UpdateFields{..})`,
  never by the layout; DATE/TIME in layout comes from `LayoutOptions.now` (the binding passes
  `Env::now()`).
- **Hyphenation skips** are keyed by `(ParaId, offset)` (TS `hyphenationSkip(para, offset)` by
  paragraph object).
- **Callbacks into the shell during layout** (plugin `InlineRenderer.measure`): a layout-side trait
  `InlineMeasurer` implemented in the binding by a synchronous wasm import (cheap; no re-entry into the
  session); on .NET no renderers exist (fallback text). Specified in the uo-docx-layout plan.
- **Previews** (TOC/header-footer/style gallery thumbnails): the binding lays out a small
  `Document` built by a session query (`Query::PreviewDocument{..}`), no oracle call.

### A5. The reader's output contract

`read_document(pkg, &ReadOptions { sources: bool, limits })` returns a `Document` that is the TS
`readFullDocument(pkg, { sources })` output in the A2 shapes:

- every field the TS sets, with the same value; keys the TS stores as `undefined` carry the cleared
  bit; stored `NaN`/`±∞` as `JsNumber::Num`; numbers through uo-jsnum (`parse_int`, `to_number`,
  `parse_float`, `round`, fdlibm trig in `drawing.ts`);
- text in UTF-16 from the UTF-8 XML (uo-opc already maps bad references to U+FFFD);
- maps and lists in TS order (`IndexMap` where the TS uses `Map`; ascending for integer-like keys of
  plain objects);
- **no session work**: no `markSectionBreaks`, `syncRunRPr`, note-run marking, DATE text or baseline
  (those are the D2a install, in TS order);
- per-read counters for the TS process-global ones (`wrapIds`, `blockSdtKeys` → keys relabelled in the
  digest); `ParaId`/`TableId`/`DrawingId` allocated in document order (not observable; relabelled);
- relationship targets resolved with `Package::resolve_target` only (uo-opc D6/R1, accepted there);
- XML through `xml_part` with the `docx-pos` preset (positions for sources), depth 1,000; nesting
  past 64 as text (G20);
- errors: `DocxError` values; the TS `try {…} catch {}` sites keep their partial results.

### A6. The canonical model digest (parity)

- **TS side** (`uo-testkit/js`, Node 24.15): a generic walker over the TS objects: object keys
  sorted; an own key with value `undefined` emitted as `U`; numbers by f64 bits (NaN canonical,
  `-0` kept); strings as UTF-16 code units; `Uint8Array` by length + SHA-256; `Map` entries in
  iteration order; arrays in order; aliased views (cell `paragraphs`/`tables` vs `blocks`,
  `headers` vs `headerBlocks`, notes, textbox `paras` vs `blocks`) emitted once from the block view
  plus a boolean "views consistent by identity"; process-global ids relabelled by first appearance;
  `SourceStore` raw strings by content hash, positions `at` converted UTF-16 → UTF-8 with
  `utf8Offsets`. A separate, **non-compared** facet records the key insertion order of the props
  objects (input to G23).
- **Rust side**: a `Canon` trait implemented with a `macro_rules!` per type listing the fields under
  their TS names (no proc-macro crate), emitting the same stream; ids are not emitted.
- **Facets** per frontier (`props`, `parts`, `drawing`, `paragraph`, `full`, `sources`), so a
  mismatch points at one frontier and one input; the stream is the uo-testkit `Stream` encoding, the
  capture the `.cap` format of uo-opc.

### A7. The Sonnet D1a reader port (closed scope)

Prerequisites (Opus, part of this architecture, written before the frontiers start): the types of A2
in `model/` (≈ 260 TS types: 195 exported by docx-core outside `doc/odf/export`, plus the ones of
`ooxml/*`), `ids.rs`, `text.rs` (`Utf16`), `js_number.rs`, `FieldMask` + `spread_over`/`assign_into`,
`canon.rs` (+ the TS walker), `ReadCtx` (limits, `IdAlloc`, `MediaStore`, rels through D6, the element
adapter over `uo_opc::xml::Element`), `DocxLimits`, `DocxError`, the regex-site helper templates
(`find_tag`, `attr_value`, `element_span`, literal-value matching), the data-table generator, and the
uo-drawingml `color::read`/`resolve` + `theme::read` API (ported with uo-drawingml's Sonnet frontier).

| Frontier | Rust files | TS source | Lines |
|---|---|---|---|
| R1 | `ooxml/props.rs`, `text_effects.rs` | `ooxml/props.ts`, `text-effects.ts` (read side) | ≈ 1,060 |
| R2 | `ooxml/parts.rs`, `styles.rs`, `table_styles.rs`, `data/*` | `ooxml/parts.ts`, `styles.ts`, `table-styles.ts`, `table-styles-builtin.ts`, `word-styles.ts`, `style-names-es.ts` | ≈ 1,280 |
| R3 | `drawing.rs`, `vml_shape.rs`, `page_background.rs` (read), `watermark.rs` (read) | `drawing.ts`, `vml-shape.ts`, read parts of `page-background.ts`, `watermark.ts` | ≈ 1,010 |
| R4a | `props_map.rs` | `document.ts`: `runPropsFromRPr`, `paraPropsFromPPr`, `editBaseProps`, `revInfo`, `readSectPr`, `readBorderEdge` | ≈ 550 |
| R4b | `inline_objects.rs` | `document.ts`: images, textboxes, shapes, groups, math, form fields, `syncImageFromXml`/`syncShapeFromXml`, `blipFxImageEffects` | ≈ 800 |
| R4c | `paragraph.rs` | `document.ts`: `ReadContext`, `buildReadContext`, `readParagraph`, runs, fields (simple/complex/nested codes), bookmarks, revisions marks, `readDocument` | ≈ 960 |
| R5 | `fulldoc.rs`, `inspect.rs`, `blank.rs`, `fonts.rs` | `fulldoc.ts`, `inspect.ts`, `blank.ts`, `fonts.ts` | ≈ 1,090 |
| R6 | `source.rs` (reader side) | `source.ts`: building `ParaSource`/`SegSource`/`TableSource`/`WrapSeg`/`StorySource` during the read (signatures excluded) | ≈ 250 |

Order: R1, R2, R3 and R4a in parallel; R4b and R4c after R4a; R5 and R6 last (R6 can start with R4c).
Each frontier compiles with clippy clean against the A2 types, runs its facet of the capture
continuously, and **declares in this record** (section "Stage 3 — D1a reader", per frontier):

1. every numeric parse/rounding site and the uo-jsnum function used;
2. every regex site and its matcher (a row of the spec table), with the dynamic ones named;
3. every field that can store `NaN`/`±∞` (→ `JsNumber`) and every key stored as `undefined`
   (→ cleared bit);
4. every `Map`/object iteration order relied on;
5. every recursion and how it was made iterative or limited;
6. every module-level or process-global state and its per-read replacement;
7. every `try {…} catch {}` and the partial result kept;
8. every place two model positions share one TS object (aliasing) and how the A2 storage gives the
   same observable result;
9. every saturating/wrapping/truncating choice and every data-shape deviation from A2 (needs an Opus
   check before it lands).

## Reuse

**Inside the repository**

| Need | Copies today | Placement |
|---|---|---|
| XML, package, rels, CFB, XML writer | docx-core `xml.ts`, `package.ts`, `doc/cfb.ts`, `doc/xmlw.ts`, `odf/zip.ts` (+ sheet/slides copies) | uo-opc (done) |
| OfficeArt records, FOPT, BStore/BLIP | `doc/officeart.ts`, `slides-core/ppt/officeart.ts`, `sheet-core/xls/officeart.ts` | `uo_drawingml::odraw` (G11) |
| msospt → `prst` | `doc/preset-map.ts`, `ppt/preset-map.ts`, `xls/odraw-geom.ts`, C# `PresetMap.cs` | `uo_drawingml::msospt` (uo-drawingml G6) |
| DrawingML colour resolver | `drawingml/color.ts`, `charts/dml-color.ts`, `slides-core/theme.ts`, C# `DmlColorMods.cs`, Write's string path (`drawing.ts:259` → `resolve.ts:397`) and `serialize.ts:shapeColor` | `uo_drawingml::color` (G12, milestone 1) |
| Theme part | `ooxml/parts.ts readTheme`, `slides-core/theme.ts`, `sheet-core/theme.ts` | `uo_drawingml::theme` (G12, milestone 1) |
| Embedded fonts | docx-core `fonts.ts`, slides-core `fonts.ts`, three `deobfuscateFont` | uo-fonts `odttf` (done) + docx part writer here |
| Undo by snapshot | Write JSON, Slides `history.ts`, Sheet snaps | *(revised after the skeptic, U11)* `uo_docx::history` now; a crate when a second app's semantics converge (G5) |
| Collation | `sheet/calc/collation.ts` (used by Write) | `uo-text` (G10) |
| Docx → Markdown/HTML | TS `docx-core/export` (model path, used by the editor), TS `docxToMarkdown` → converter `docs-processing`, C# `Export` (model path) | `uo_docx::export`, model path (G14) |
| Docx reader/model in C# | C# `DocumentReader`, `Model`, `FullDoc`, `Serialize`, `DocBinary`, `Export` | replaced by uo-docx through the facade |

Not shared (checked): Word text effects (w14) vs Slides `a:` effects are different schemas; Sheet's
integer HLS `tint` is a different spec (uo-drawingml); the Word field engine, numbering, styles and
TOC are Word-only.

**Third-party**: only `indexmap` (already allowed), optionally `smallvec`. Everything else is an own
port for parity or size reasons (table in "Crate vs own port"); the evidence against `regress`/`regex`/
`icu_*` is the documented semantic differences, not measurements.

## Remaining Write work (estimate)

*(revised after the skeptic, U20, and the hybrid mode)* Behaviour source lines on the Write 0.10 path
(TS, plus C#-only), counted in this worktree.

| Crate / part | Behaviour source | Lines | State |
|---|---|---|---|
| uo-opc (Write share) | `xml.ts`, `package.ts`, `doc/cfb.ts`, `doc/xmlw.ts`, `odf/zip.ts` (+ fflate subset, C# Opc*) | 730 | **done** (gate 3) |
| uo-fonts | `shared/fonts` minus `ooxml.ts` (1,354) + `docx-layout/typography` font/hyphen (1,367) | 2,720 | **done** (gate 2) |
| uo-jsnum A | V8 numerics (no Write TS lines) | — | **done** (gate 2) |
| uo-drawingml | `shared/drawingml` minus `contour`/`shape-styles` (1,890) + blip effects of `images.ts` (≈ 200) | 2,090 | in progress (stage 3) |
| uo-jsnum B | V8 formatters, `parseFloat` (no Write TS lines) | — | in progress |
| **uo-docx D1a** model + reader | | 6,800 | milestone 1 |
| **uo-docx D1b** lossless writer + fonts `ooxml` | | 4,180 | milestone 2 |
| **uo-docx D2a+D2b** session, undo, ops | docx-core ops 5,150 + editor model files ≈ 5,500 + `universal-write.ts` model ≈ 10,500 + contour/shape styles 170 | ≈ 21,300 | milestone 2 |
| **uo-docx D3** Editing API | C# | 4,750 | separate |
| **uo-docx D4** `.doc` (+ shared odraw/msospt 580) | | 5,670 | separate |
| **uo-docx D5** export, ODF, compare (minus the shell element) | | 3,320 | milestone 2 |
| uo-docx-layout | `docx-layout` minus fonts, PDF, canvas, images, tree | ≈ 12,000 | milestone 1 (Opus) |
| uo-render | `tree.ts` + the display-list codec (new design) | 275 | milestone 1 |
| uo-pdf | `pdf-writer/*` 840 + PDF painter 953 | 1,790 | milestone 1 |
| uo-image | codecs of `images.ts` ≈ 400 + `emf.ts`/`wmf.ts` 781 | ≈ 1,180 | milestone 1 |
| XPS export | `editor/xps.ts` + the XPS painter paths | ≈ 350 | milestone 2 |

- **Behaviour source**: done ≈ 3,450 (5%), in progress ≈ 2,090 (3%), **remaining ≈ 61,600 (92%)**
  of ≈ 67,100 lines. uo-docx ≈ 46,000 (69% of the total); uo-docx-layout ≈ 12,000 (18%).
  **Milestone 1** is ≈ 22,000 lines (D1a, uo-docx-layout, uo-render, uo-pdf, uo-image: 36% of the
  remaining), most of it Opus (layout). By model: ≈ 23,000 of the remaining lines are Opus-only
  (uo-docx-layout, D1b core, D2a, find/paste), ≈ 39,000 Sonnet-portable.
- **Not behaviour source, not in the percentages** (skeptic U20): the regex spec work at the real
  count (U7), the TS canonical walker and the `Canon` impls for ≈ 260 types, the Playwright harness
  with the uo-jsnum math override (U5), the C# differential harness, the generated `.doc`/`.odt`
  corpora (U15), the `LayoutOracle` and the editor-facing layout API (U1, U10), the identity and
  version machinery (U3, U12), the bindings (uo-wasm Write session + codec ≈ 3k Rust and ≈ 2k TS;
  uo-ffi + C# facade ≈ 1.5k Rust and ≈ 3k C#), stages 5–6, BrowserStack, maintainer latency, G19.
- **TS shell switch**: `universal-write.ts` keeps ≈ 12.5–13k lines, of which ≈ 6–8k are touched; the
  canvas painter (1,816) is rewritten to decode the display list; the viewer (2,396) ≈ 0.8k touched;
  the chrome (≈ 14.4k) ≈ 1–2k touched; the compare element re-plumbed to session queries; the legacy
  canvas layout removed or replaced (G19).

**Time, through gate 2, before maintainer latency.**

| Basis | Hours | Measured or assumed |
|---|---|---|
| Floor: the uo-fonts rate (2.7k TS lines through gate 2 in ≈ 4.7 h wall ≈ 560 lines/h) × 61.6k | ≈ 110 | rate **measured** on a closed-scope module; applying it to Write is an assumption |
| Working band without the hybrid mode (skeptic U20: D1b/D2 at 1.5–2× the uo-fonts rate, plus the harness work above) | **150–180** | multiplier **assumed** |
| With the hybrid mode | **≈ 105–145** (≈ 125 midpoint) | see below |

How the hybrid figure is built:
- **Measured** (process metrics of the records): the port itself is a small share of a module's wall
  time — uo-opc ≈ 19% (port 33 min of ≈ 176 through the gate-2 decisions: plan + skeptic 17, stage 4
  86, decisions 40), uo-fonts ≈ 24% (port 67 min of ≈ 280: plan + reuse + skeptic + revision 53,
  stage 4 ≈ 160 with the parity verifier as the long pole); Sonnet ports had 0 parity rounds in both
  Sonnet modules, and their stage-4 defects came from plan gaps.
- **Assumed**: with verification running continuously and overlapping the next frontier's port, no
  per-module plan/skeptic/gate-1b for the Sonnet parts (seven briefs not written) and one gate per
  milestone, the Sonnet-portable share (≈ 63% of the lines, ≈ 95–113 h of the band) loses 25–40% of
  its wall time and the Opus share (≈ 37%, ≈ 55–67 h; layout and D2a keep their rigor) 10–15%.
- **Not reduced**: the harness and corpus work, the Opus design work, maintainer latency, stages 5–6.
- The first Write-specific rate comes from the D1a frontiers R1–R6: their wall times and parity
  rounds go into "Process metrics", and this table is re-based on them.

## Milestone-1 gate — consolidated questions (G1–G27)

*(revised after the skeptic: one list; the skeptic's revisions and G21–G27 merged; final
recommendation per question.)* **When**: **A** = decide before the D1a port starts; **L** = before
the uo-docx-layout plan starts (still inside milestone 1); **M2** = with milestone 2 (D1b/D2/D5); **S**
= with the separate D3/D4 work. Decisions already taken by the maintainer are marked.

| # | Question | Final recommendation | When |
|---|---|---|---|
| G1 | One crate `uo-docx` with modules + features (`session`, `editing-api`, `doc`, `odf`, `export`, `compare`) vs a crate per sub-module | **One crate, features**; sub-crate only for compile time | A |
| G2 | Read path first | **Decided** (hybrid mode, milestone 1). Conditions (U10): the D1a types designed with the D2a model (done in "Architecture (milestone 1)"); the editor-facing layout API in the uo-docx-layout plan before milestone 1 closes; milestone 1 internal or a preview, **0.10 released at the editor switch** — confirm | A (confirm) |
| G3 | Representation: the TS flat shape (paragraphs + tables anchored before paragraph N; edit contexts select the story), for every story incl. table cells, header/footer sets, notes, textboxes, stored once with the `CellView` stale/reconcile rule (A2) vs a block tree | **Flat, stored once** (A2); block tree a stage-5 candidate | A |
| G4 | Undo: (a) literal JSON, (b) structural sharing + explicit normalisation, (c) inverse ops; with the cleared mask, `JsNumber::Null` after undo, normalisation at restore, and the textbox-wrapper loss **fixed by name** (U4) | **(b) with all four** (A2, A3). The types part (mask, `JsNumber`) is needed now | A (types) / M2 (mechanics) |
| G5 | `uo-history` crate vs module | **`uo_docx::history` module**; a crate when Slides/Sheet converge (U11) | M2 |
| G6 | Text: (a) UTF-16 storage in the model, positions native, lone surrogates as in the TS (U2); (b) code-point editing, accepted by name | **(a)** — the TS keyboard creates lone surrogates; (b) would diverge on every non-BMP script; cost 2 B/char | **A** |
| G7 | Process/instance state explicit: per-read counters (`wrapIds`, `blockSdtKeys`), per-save `WriteCtx` (incl. `rawSrc`), per-session counters reset per document (`nextBmId`, `nextSectionId`, `docToken`; TS carry-over accepted by name), clipboard ring in the binding, `RichClip` identity as session id + generation; digests relabel ids | **Yes** (U17) | A (reader counters) / M2 |
| G8 | `Env` per session (clock with local date parts, RNG, locale); captures with seeded `Math.random` and fixed `Date` | **Yes** | A (capture) / M2 (API) |
| G9 | No regex engine: hand matchers per site from a spec table (real count ≈ 550–740), literal-value templates for the ≈ 55 dynamic sites (TS metacharacter/`SyntaxError` behaviour accepted by name), bounded `fieldCodeXml` matcher, differential against JS `RegExp`; D3 regexes against .NET; own find matcher | **Yes** (U7, U13) | A (reader sites) |
| G10 | New shared crate `uo-text` (collation; Unicode tables pinned to Node 24.15) | **Yes** | M2 |
| G11 | OfficeArt into `uo_drawingml::odraw` (feature `legacy`) next to `msospt` | **Yes** (alternative: crate `uo-odraw`) | S |
| G12 | Colour resolver (`color::read`/`resolve`) and theme reader in uo-drawingml, structured `DmlColor` in the model, no `"RRGGBB|mods"` string, Write quirks as a named variant until stage 5 | **Yes** | **A** |
| G13 | D3: own mutable DOM on the strict parser, saved parts byte-equal to C#, Rust-first API with the clickCompliance names in the facade, views as (handle, `NodeId`); host callbacks two-phase (or `[UnmanagedCallersOnly]` with exception containment); `Regex`-taking APIs dropped from the facade; .NET-semantics table | **Yes; two-phase callbacks** (U13) | S |
| G14 | Standalone docx → Markdown/HTML through the model, not the converter path | **Yes**; reference = TS `toMarkdown(exportDocFromFullDocument(readFullDocument(…)))` | M2 (or milestone 1 if the server needs export) |
| G15 | Shell boundary: one `UoWriteSession`; hot-path exports; `exec`/`query` through a schema-generated codec shared with the uo-render display list; `ChangeSet` with `StoryChange`s and the full event table; paste tree with CSSOM longhands, raw `style`, comments; context-entry commands | **Yes** (U16). The codec generator is shared with uo-render, so its choice is needed when uo-render starts | L (codec) / M2 |
| G16 | Models per part | **The skeptic's verdict table** (adopted in "Models") | A |
| G17 | Editor capture: Playwright Chromium over the frozen bundle, uo-jsnum math override, checkpoints after timers settle, private entry points on an unminified bundle; **all other captures in Node 24.15 x64** | **Yes** (U5, U16) | A (Node-only for D1a) / M2 (harness) |
| G18 | Corpora: Word-generated `.doc` and `.odt` from the oracle docx (`doc-lab/gen-fixtures.ps1`), LibreOffice `.odt`, the local third-party `.doc` corpus; generators and hashes committed, files local; the D1a attribute-quirk corpus | **Yes** (U15) | A (D1a quirk corpus) / S, M2 |
| G19 | The TS legacy canvas layout (web/draft/outline views, HF/textbox composition) cannot survive the switch: port the views into uo-docx-layout or drop them | **Decide in the uo-docx-layout plan**; excluded from the estimate | L |
| G20 | `max_nesting` 64; content past it read as plain text in place (not rejected); iterative walks; one wasm test at the limit through read → write → layout → digest | **Yes** (U18) | **A** |
| G21 | Layout-pass mutations: `Session::settle()` for `syncSections`/`syncRunRPr`; `LayoutOracle` (field values, pages, hyphenation, font usage) implemented by uo-docx-layout in the binding; the layout-dependent D2b family after uo-docx-layout; the "layout failed → legacy + `exitCell`" fallback dropped by name | **Yes** (A3, A4). The trait shape is needed by the layout plan | L |
| G22 | Explicit identity ids (`ParaId`, `TableId`, `DrawingId`) with a per-op "in place vs new object" rule; `Arc::ptr_eq` only as a content-unchanged fast path | **Yes** (A1, A3) | **A** |
| G23 | Pristine signatures order-insensitive; accepted by name if the harness finds no byte difference, else an order tag | **Yes** (U6) | M2 |
| G24 | Media dedupe by full byte equality (writer and `.doc` converter) | **Yes** (U8) | M2 / S |
| G25 | Two-phase commands for codec/clipboard/async work (`Outcome::NeedsJobs` + `complete`); shell bytes fed to both sides | **Yes** (U9) | M2 |
| G26 | Style/numbering version stores collected with the history, inside its byte cap | **Yes** (U12) | M2 |
| G27 | D3 ships as a hybrid NuGet (Rust Editing API + C# reader/layout/PDF) before the Rust PDF path, or waits for it | **Maintainer.** Planner's view: ship the hybrid with the C# reader classes made internal, and have the host validator and the Rust API resolve targets with the same D6 rule (already required), so "the part checked is the part opened" holds within the Rust side; the two parsers only meet in the PDF path, which reads the sanitized bytes | S |

**Needed before the D1a port starts (A):** G1, G2 (confirm the release condition), G3, G4 (types:
cleared mask, `JsNumber`), G6, G7 (reader counters), G8 (capture determinism), G9 (reader regex
sites), G12, G16, G17 (Node-only captures), G18 (attribute-quirk corpus), G20, G22.
**Before the uo-docx-layout plan (L):** G15 (codec), G19, G21.
Everything else waits for its sub-module.

## Analysis skeptic (verified in the code)

Stage 2, Opus, 2026-11-03. Every claim below was checked in the frozen TS (`write/packages/**`,
`slides`/`sheet` where named), the C# (`dotnet/src/UniversalOffice.Docx/**`) and the clickCompliance
host; regex sites were counted with the TypeScript parser (regular-expression literal tokens and
`new RegExp`), not by grep. Line numbers are of the `rust-core` worktree. Claims of the plan that held
are not repeated; the counts that held are listed in U20.

| # | Finding (evidence) | Blocking | Proposed decision |
|---|---|---|---|
| U1 | **The layout pass mutates the model, so the session is not layout-free.** `layout()` (universal-write.ts:12448–12465) runs `syncSections` (11004–11019: new `sectionBreak.id` from `nextSectionId`, `p.props` replaced, header/footer refs migrated) and `syncRunRPr` (16068–16081); `layoutEngine` (16144–16166) runs `syncRunRPr` and, on a layout exception, falls back to the legacy layout and calls `exitCell()` (state change on layout failure); `refreshFieldCache` (6457–6466) runs a **full `layoutDocument` with `onField`** and writes the values into runs (`setFieldResult`, 6471–6500) — 12 call sites (update fields, TOC, cross-references, captions); a fast layout schedules a full refresh 1.5 s later (16270–16273, 16393); hyphenation relayouts (17337–17353). The plan says uo-docx does not depend on layout and gives D2b only D2a and uo-text as dependencies; the D2 capture (state after each step) includes these mutations. | **yes** | New **G21**: (a) the layout-independent normalisations (`syncSections`, the rpr inheritance of `syncRunRPr`) become an explicit `Session` step run at the end of every `exec`/`undo`/`redo` (where the TS `refresh()` runs), each one named in the D2a brief; (b) `trait LayoutOracle` in uo-docx (field values, page of a position, preview pages, hyphenation points) implemented in the binding by uo-docx-layout; (c) the D2b family references/TOC/fields/captions/galleries/hyphenation starts after uo-docx-layout gate 2; (d) the layout-failure fallback (`exitCell`) is dropped, accepted by name (Rust layout errors are values). |
| U2 | **Lone surrogates come from the keyboard.** Backspace deletes one code unit (`deleteRange(offset - 1, offset)`, universal-write.ts:9809–9812) and caret-left moves one unit (9964): after an emoji (or any non-BMP character) the TS model holds a lone high surrogate, which the save writes through `TextEncoder` as U+FFFD. G6 and risk 1 assume lone surrogates "cannot exist" and that offsets snap to the pair start: every script touching non-BMP text diverges in offsets from the first Backspace on. | **yes** | G6 revised (maintainer): (a) **faithful** — run text in the model stored as UTF-16 code units (`Box<[u16]>`), positions native (no per-op conversion; uo-docx-layout indexes UTF-16 too), UTF-8 only at the XML boundaries with the `TextEncoder` rule (lone → U+FFFD); cost 2 B/char (Latin 2 vs 1, CJK 2 vs 3); or (b) fix — delete/move by code point (Word's behaviour), accepted by name, non-BMP scripts excluded from D2 equality. Recommend (a). |
| U3 | **Object identity is load-bearing beyond the plan's list.** `reconcileBlocks` (engine-doc.ts:229–261) places paragraphs by identity (`p === flat[i]`, `Set` of paragraph objects); `TableCell` keeps three aliased views of one content — `paragraphs`, `tables`, `blocks` (fulldoc.ts:37–50) — re-aliased after every undo by `TOps.relink` (table-ops.ts:1166–1181, universal-write.ts:9614); `SourceStore.txbx` is a `WeakMap` keyed by textbox objects (source.ts:181) read at save (serialize.ts:220); per-save `memo`/`emittedShapes`; `RichClip.doc` (document identity) and `src: WeakRef` (718–730). `Arc::ptr_eq` cannot stand in: copy-on-write changes the pointer on an in-place mutation, where the TS object keeps its identity. | **yes** | New **G22**: explicit identity ids (`ParaId`, `TableId`, `TextboxId`: per-session `u32`) in the D1a types; the D2 briefs carry, per TS construction site (`{ ...p }`, literals, `splice`), whether the op mutates in place (keeps the id) or builds a new object (new id); undo normalisation assigns fresh ids (= the JSON copies); a cell stores its blocks once with the paragraph view as indices, plus the TS "stale until reconcile" rule. Designed with D2a before D1a stage 3 (U10). |
| U4 | **The JSON round trip has more observable effects than G4 lists, one of them data loss.** (a) `Object.assign(props, patch)` stores explicit `undefined` keys (model-ops.ts:366; `CLEARED_RUN`, universal-write.ts:890–896); they override when stored props are spread after another object (paste: `{ ...DEFAULT_RUN_PROPS, ...base, ...r.props }`, 5536) and vanish after an undo — `Option<T>` storage cannot give both behaviours; (b) a `NaN` stored by a reader becomes `null` after undo, and `null` passes `!== undefined` and is 0 in arithmetic, unlike `NaN`; (c) after an undo the textbox object is a copy, the `txbx` lookup misses and the textbox's block `w:sdt`/`w:customXml` wrappers are dropped at its next save (serialize.ts:219–222); (d) `imgPool` keeps every image ever snapshotted (9401–9402, never shrinks). | **yes** | G4 (b) stays, with: `RunProps`/`ParaProps` carry a `cleared: FieldMask` (explicit `undefined`) set by `Patch::Clear`, honoured by one `spread_over` helper, dropped by normalisation; the D1a quirk capture lists every field that stores NaN/±∞, each one either a `Null` state or accepted by name; (c) **fixed** (wrappers survive undo), accepted by name, the harness flags it; normalisation runs at restore, not at snapshot (the snapshot stays O(touched)). |
| U5 | **The D2 harness runs in Chromium, which uo-jsnum G2 (approved, uo-jsnum.md:570) excludes as a reference for any output that goes through math.** Model paths call `Math.sin/cos/tan/atan2` (drawing.ts ×10 — the D1a reader —, picture-ops.ts ×8, universal-write.ts ≈ 47 incl. `hypot`, part of them in painting) and `Math.pow` (table-formula.ts ×3). | **yes** (G17 wording) | G17: the harness page replaces `Math.sin/cos/tan/atan2/pow/hypot/exp/log` with the uo-jsnum wasm exports (proven equal to Node 24.15) by `addInitScript`, recorded in the capture metadata; the D1a, D1b, D4 and D5 captures run in Node 24.15 x64 as G2 requires. |
| U6 | **Pristine signatures depend on key order.** `runSig`/`propsSig`/`tableSig` hash `JSON.stringify` of the run, the paragraph props and the whole table (source.ts:141–149, 329) and decide raw vs re-serialized output (`pristine`, 294); `lossless.ts` compares by `sameJson` too (239–292). The plan's key-order list names only `sameJson`/`sameRPr` of model-ops, `bgKey` and the note settings. | no | Rust signatures over an order-insensitive canonical encoding; the harness counts the steps where the TS order-sensitive signature disagrees with the order-insensitive one **and** the saved bytes differ; accepted by name if that count is 0 on the corpus, else an order tag on the affected objects. |
| U7 | **G9 under-counts the regex sites, and ≈ 55 patterns are built at run time from document data.** Parser count: docx-core 350 literals + 34 `new RegExp`; editor model files 156 + 13; universal-write.ts 181 + 7 (part stays in the shell) → ≈ 550–740 sites, not ≈ 350; 14 with the `u` flag. Dynamic patterns interpolate unescaped values (`Id="${prev.rid}"`, page-background.ts:189; `w:abstractNumId="${id}"`, numbering-store.ts:149; `Id="${rid}"`, universal-write.ts:8521): metacharacters change the match or throw `SyntaxError`. `fieldCodeXml` builds `^lit([\s\S]*?)lit…$` from the field code (serialize.ts:627–628): k lazy groups, O(n^k) backtracking on untrusted input. | no | G9 revised: shared helper templates for the dynamic sites (element by tag/attribute value, value matched as a literal — TS metacharacter/`SyntaxError` behaviour accepted by name); `fieldCodeXml` as a bounded lazy-group matcher with a work budget; the spec table and the estimate use the real count. |
| U8 | **Media are deduplicated by sampled bytes.** `mediaRelId` takes an existing part whose length, every 509th byte and last byte match (serialize.ts:170–175); the `.doc` converter keys media by `ext:length:FNV` of ≤ 4,096 sampled bytes (doc/docx-writer.ts:824, 1254–1259). Two different images of the same length that agree on the samples become one part (the wrong picture) — likely with the uncompressed DIBs of `.doc`. | no | New **G24**: full byte equality in Rust, accepted by name (TS false positives only); a quirk pair (same length, differing off-sample) in the D1b and D4 corpora. |
| U9 | **Some commands need browser codecs and are async.** `compressImages` resamples with `OffscreenCanvas` and re-encodes with `convertToBlob` (JPEG 0.85, universal-write.ts:15669–15697); `changeImage`, `changeImageFromClipboard`, `setShapePictureFill`, `insertImage` and `setImageAdjust` are async; `renderClipPng` lays out and paints the clip for the clipboard (5604–5621); `paste()` reads the system clipboard (4991–5009). The plan's `exec` is synchronous with no protocol for these. | no | New **G25**: two-phase commands (Rust returns the decode/encode jobs, the shell runs them, Rust applies the bytes); parity feeds the shell's bytes to both sides, and re-encoded bytes are not a parity output. |
| U10 | **"Read-only first" freezes two APIs the editor reshapes.** The representation decisions of U2–U4 (UTF-16 storage, identity ids, cleared masks, cell views, rpr provenance) live in the D1a types that uo-docx-layout reads. The layout API must also serve the editor: incremental relayout (`tryFastLayout`/`tryFastTable`/`installEngineResult`, 16235–16552), field values (U1), gallery and TOC previews (`paintTocTemplate` 7086, `paintHeaderFooterTemplate` 18484, `paintStyleThumb` 20169), hyphenation, and the plugin `InlineRenderer.measure` callback (docx-layout inline-renderer.ts:35). | no | G2 yes, on conditions: the D1a type brief and the D2a session model are designed together at gate 1b (Opus) before D1a stage 3; the uo-docx-layout plan specifies the editor-facing API before the read-only milestone; the milestone is internal (or a preview) and the 0.10 release stays at the editor switch — maintainer. |
| U11 | **`uo-history` would share less than it seems.** Write snapshots the state **before** a step and undo pushes the **current** state to the other stack (9570, 9586), so changes made without a snapshot (`quietChange`, layout mutations) land in redo; states are partial (`deep`, `parts`); `dirtiedAt`/`discardSnapshot`; never clean after an undo (9561). Slides (`history.ts:44–60`): before/after pairs, `savedPos` (undo returns to clean), limit 100. Sheet: before/after sheet snaps. The common part is a capped deque. | no | G5 revised: `uo_docx::history` (module) with Write's semantics and `PVec` private; a crate when Slides/Sheet port and their semantics converge (crate rule: no second user yet). |
| U12 | **Version stores grow without bound.** `DocNumbering` and `DocStyles` keep every XML version forever ("as versões ficam todas", numbering-store.ts:50, 140–144; styles.ts:85–89); history steps hold only the indices (`sv`, `nv`). | no | A monotonic version counter → `Arc<str>` map, entries dropped when no live step or the current state refers to them; inside the history byte cap; M5 test with 10,000 list toggles. |
| U13 | **D3 calls back into host code, and the C# has its own semantics table.** `DocxSanitizeOptions.IsCustomInlineValid: Func<string, string, bool>` (DocxSanitizer.cs:52) is set by clickCompliance in both sanitizations (`DocxEditingOptions.cs`); `DocxCustomInlines.UnwrapInvalid(doc, Func)` (179); `DocxImporter.ImportBody(Func<XElement, XElement>)` (61); `DocxText.Replace(doc, Regex)` and `ParagraphText.Find(Regex)` take a .NET `Regex` (DocxText.cs:53, ParagraphText.cs:107). .NET semantics a Sonnet translator would otherwise decide: `OrdinalIgnoreCase`, `int.TryParse` number styles, enumeration order of the `HashSet<XNamespace>` that orders the added `xmlns` (DocxDocument.cs:542–575), the `XmlWriter` declaration (`encoding="utf-8"`), .NET regex sites (Colors.cs:9, DocxPictures.cs:168, DocxDocument.cs:177), `System.Text.Json` in `CustomInlineTag`. G9's "differential against JS `RegExp`" does not apply to D3. | no | G13 revised: the validator as a two-phase API (list `(type, json)` → host verdicts → apply) or an `[UnmanagedCallersOnly]` callback with exception containment — maintainer; the `Regex`-taking APIs dropped (unused by clickCompliance) or two-phase in the facade; the D3 brief carries a .NET-semantics table and D3 regexes are differential against .NET 10. |
| U14 | **Shipping D3 first makes a hybrid package.** The server PDF path is `UniversalOfficeWordToPdfConversionService` → C# reader, `DocxLayout`, `DocxPdf`. Group (i) before group (iii) ships the Rust Editing API next to the C# reader/layout in one NuGet: two parsers of the same docx in one process, and "the library opens exactly the part the sanitizer checked" then spans both. | no | New **G27**: ship the hybrid (C# reader classes internal until uo-pdf) or hold D3 for group (iii) — maintainer. |
| U15 | **The corpus gaps can be filled with the repo's own tools.** `write/tools/doc-lab/gen-fixtures.ps1` already drives Word (COM, `SaveAs2` format 0 → `.doc`, 16 → docx); run over the 2,742 oracle docx it gives a Word-written `.doc` corpus; `SaveAs2` format 23 gives `.odt` from Word, and LibreOffice headless a second producer. Today `readOdt` (odf/index.ts:88, 807 lines) is covered only by `writeOdt` output. | no | G18 extended: generated `.doc` and `.odt` corpora (generator and hashes committed, files regenerated locally); targeted mutations for what Word does not write (fast-save piece tables, other producers). |
| U16 | **Boundary details the D2a brief must carry.** Events: ≈ 38 `uw-*` types; model-derived beyond the six of G15: `uw-revisions-changed`, `-comments-changed`, `-bookmarks-change`, `-styles-change`, `-tbmode`, `-dirty-change`, `-caption-settings`, `-track-changes`, `-sort-error`, `-pasted`, `-custom-inline-inserted`, `-blocked`, `-theme-change`; the cancelable `uw-before-input` precedes the snapshot of every keystroke (9737–9743). Paste: `paste-html.ts` reads the inline CSSOM (`el.style.fontWeight`…, shorthand-expanded longhands, colours as `rgb()`), the raw `style` attribute (mso-list), comment nodes (171, 227) and `:scope` selectors (universal-write.ts:20549) — not a computed style. Cell, note and textbox editing are entered through private methods (`enterCell` 18738, `enterNote` 18798, `enterTextbox` 2700), not the public API. | no | G15/G17: the event table in the D2a brief; the paste tree carries CSSOM longhands + raw `style` + comments (Chromium's CSSOM is the reference); `Command`s for the context entries; the harness calls TS-`private` methods (reachable at run time) on an unminified bundle. |
| U17 | **More instance and module state.** `nextBmId` (6105, 6167–6171) is not reset on load (`resetSections`, 10955–10963), so bookmark ids depend on the documents opened before in the same element; `nextSectionId`; `docToken`; `picture-xml.ts:353` module `rawSrc` (set/reset 475/551, 955/988). | no | G7: per-session counters (reset per document, accepted by name for the TS carry-over); digests relabel them; one element per page in the harness. |
| U18 | **G20 leaves the outcome open.** The TS reads deeper nesting; with the limit, the viewer either rejects the document or drops content. Each level costs several Rust frames (table → row → cell → blocks → paragraph → run → textbox) in reader, writer, layout, digest, export and compare. | no | G20: state the outcome (recommend: content past depth 64 read as text, accepted by name, rather than rejecting the file) and one wasm test at the limit through read → write → layout → digest with the configured stack size. |
| U19 | **The 64 KiB/keystroke budget holds only if every `EditState` collection is persistent.** `bookmark_paras: Vec<(Box<str>, u32)>` clones every name per snapshot (a 500-page document with a TOC has ≈ 1–2k `_Toc` bookmarks, ≈ 50 KB); `table_anchors` is O(tables); a flat `PVec` chunk list costs O(n/64) pointers per snapshot (≈ 2 KB at 15k paragraphs), not size-independent. A byte cap through `HeapSize` double-counts shared chunks. | no | Anchors and bookmarks behind `Arc`/`PVec`; step cost = bytes not shared with the next state (pointer comparison when the next step is pushed); the budget document generated with many bookmarks, tables and sections. |
| U20 | **Counts and the estimate.** Verified: docx-core 24,653 lines / 72 files; universal-write.ts 23,473 (≈ 1,143 methods, 141 `snapshot()` calls); editor model files 6,000; D1a 6,798; D1b 3,836 + 345; ops 5,147; C# `Editing` 4,699 / 26 files; 2,742 oracle docx; 11 `.doc`; 0 `.odt`; 195 exported types in docx-core (outside doc/odf/export) + 66 in universal-write.ts. Off: `compare` 1,756 includes the 702-line `<universal-write-diff>` element (shell; its `loadDocuments` takes TS `FullDocument`s, so it needs a document query after the switch): D5 ≈ 3,320. Not in the ≈ 110 h: the regex doubling (U7), the TS canonical walker and `to_ts_canonical` for ≈ 260 types, the Playwright harness and math override (U5), the C# differential harness, the generated corpora (U15), the `LayoutOracle` and the editor-facing layout API (U1, U10), the identity/version machinery (U3, U12), seven gate-1b briefs, stages 5–6, BrowserStack, maintainer latency, G19. The 560 lines/h comes from a closed-scope module. | no | Present ≈ 110 h as a floor and 150–180 h through gate 2 as the working band (D1b/D2 at 1.5–2× the uo-fonts rate) until the D1a process metrics give a Write-specific rate. |

### Model verdict per sub-module (skeptic)

The comparison in uo-fonts.md says Sonnet ports are faithful and that the risk is a decision the plan
left open. So the question per part is whether the gate-1b brief can close every decision.

| Part | Plan | Verdict |
|---|---|---|
| D1a types, colour resolver/theme API | Opus | **Opus**, and designed together with the D2a model (U2–U4, U10) |
| D1a reader translation | Sonnet | **Sonnet**, once the types, the quirk corpus and the Node-only capture (U5) exist |
| D1b | Opus (all) | **Split**: Opus for `source.ts`, `lossless.ts`, signatures (U6), `WriteCtx`, dynamic regexes (U7), fonts `ooxml.ts`; **Sonnet** for the element emitters (`serialize.ts` string building, `table-xml`, `settings-xml`, `hf-ops`, `page-background`, `watermark`) inside the Opus-defined `WriteCtx` and helpers |
| D2a | Opus | **Opus** (U1, U3, U4, U9, U11, U16 are all here) |
| D2b families | Sonnet | **Sonnet** only after D2a fixes the `Command` skeleton (one owner of the enum and `Session`, one module of `impl Session` per frontier), the per-op identity rule (U3), the two-phase protocol (U9) and the event table (U16); references/TOC/fields/captions/galleries/hyphenation wait for the `LayoutOracle` (U1). Find matcher and paste boundary: **Opus** |
| D3 DOM, strict reading, serializer | Opus | **Opus**, plus the callback/two-phase design (U13) |
| D3 API translation | Sonnet | **Sonnet** with the .NET-semantics table (U13); sanitizer reviewed by security (Opus) |
| D4 | Sonnet | **Sonnet**; the `odraw` API shape across the three copies (Word: PlcSpa/`foDelay`; Slides: ClientAnchor; Excel: OBJ order) is a short Opus brief |
| D5 | Sonnet | **Sonnet**; the compare element stays in the shell |

### Revisions to "Questions for the maintainer (gate 1)"

| # | Revision |
|---|---|
| G2 | Yes, with the conditions of U10: D1a types and the D2a model designed together at gate 1b; the editor-facing layout API specified in the uo-docx-layout plan before the read-only milestone; the milestone internal or a preview, 0.10 at the editor switch. |
| G3 | Faithful also for table cells: one storage with the `paragraphs`/`tables`/`blocks` views and the `reconcileBlocks` rule (U3). |
| G4 | (b), with the cleared-field mask, the NaN list, normalisation at restore, and the textbox-wrapper loss fixed by name (U4). |
| G5 | `uo_docx::history` as a module now; a crate when Slides/Sheet converge (U11). |
| G6 | Maintainer: UTF-16 storage in the model (recommended) or code-point editing accepted by name (U2). |
| G7 | Adds `nextBmId`, `nextSectionId`, `docToken`, `rawSrc`, and `RichClip`'s document identity and `WeakRef` (session id + generation) (U3, U17). |
| G9 | Real count ≈ 550–740 sites; dynamic patterns as literal-value templates; bounded `fieldCodeXml` matcher; D3 regexes against .NET (U7, U13). |
| G13 | Plus the validator callback design and the `Regex`-taking APIs (U13). |
| G15 | Plus the full event table, the CSSOM paste tree and the context-entry commands (U16). |
| G16 | As the verdict table above (D1b split; D2b gated on D2a's skeleton and on the `LayoutOracle`). |
| G17 | Plus the uo-jsnum math override in the harness page, Node-only captures elsewhere, checkpoints after timers settle (queued refresh, idle relayout, hyphenation, fonts), private entry points allowed (U1, U5, U16). |
| G18 | Plus Word-generated `.doc`/`.odt` corpora and a LibreOffice `.odt` producer (U15). |
| G20 | Plus the outcome at depth 65 and the wasm stack test (U18). |
| **G21** | Layout-pass mutations move into the session; `LayoutOracle` trait; the layout-dependent D2b family after uo-docx-layout gate 2 (U1). **Yes** |
| **G22** | Explicit identity ids with a per-op "new object vs in place" rule; `Arc::ptr_eq` is not identity (U3). **Yes** |
| **G23** | Pristine signatures order-insensitive, accepted by name if the harness finds no byte difference (U6). **Yes** |
| **G24** | Media dedupe by full byte equality (U8). **Yes** |
| **G25** | Two-phase commands for codec/async work; shell bytes fed to both sides (U9). **Yes** |
| **G26** | Version stores collected with the history and inside its byte cap (U12). **Yes** |
| **G27** | D3 ships as a hybrid NuGet (Rust Editing + C# reader/layout/PDF) or waits for group (iii) (U14). **Maintainer** |

## Verification (gate 2 — per sub-module)

Pending.

## Evolution

Pending.

## Final review and benchmarks (gate 3 — per group)

Pending.

## Process metrics

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| 1 | planner + reuse, umbrella plan (Opus) | (orchestrator) | ≈ 60 |
| 1 (revision) | planner: U1–U20 written in, "Architecture (milestone 1)", consolidated milestone-1 gate list (Opus) | (orchestrator) | ≈ 20 |
| M1 D1a foundation | A2 model types, A4 view and `LayoutOracle`, `ReadCtx`, A6 digest (Rust + TS walker, self-test), G9 matcher templates, data generator, G12 colour/theme API with TS differentials, M1 tests, R1–R6 brief (Opus) | one session, ≈ 3 h | ≈ 130 |
| R4a props mapping (`read/props_map.rs`) | Sonnet | one session, ≈ 30 min (plus short waits on other frontiers' transient build breakage) | ≈ 85 |
| R1 props + text effects (`read/props.rs`, `read/text_effects.rs`, props parity facet, quirk corpus generator) | Sonnet | one session, ≈ 20 min by file timestamps (plus two short waits on other frontiers' transient build breakage); no verifier round yet | ≈ 85 |
| R2 formatting parts + styles + table styles (`read/parts.rs`, `read/styles.rs`, `read/table_styles.rs`, `read/style_names_es.rs`; facets `parts`, `styles`, `table-styles`; synthetic parts generator) | Sonnet | one session; time not measured by the agent; no verifier round yet (0 mismatches on the first full replay of every facet after two small fixes) | ≈ 115 |
| R3 drawing + VML + page background + watermark read (`read/drawing.rs`, `read/vml_shape.rs`, `read/page_background.rs`, `read/watermark.rs`; facet `drawing`, `js/capture-drawing.mjs`, regex rows `r3`, unit, M1 and no-panic tests; declarations in `uo-docx-d1a/R3.md`) | Sonnet | one session, ≈ 45 min by file timestamps (plus short waits on other frontiers' transient build breakage); 0 parity rounds, rework only on the capture corpus | ≈ 105 |
| R4b inline objects (`read/inline_objects.rs`, `read/picture_format.rs`; facet `drawing-runs` with `js/capture-drawing-runs.mjs`, generated corpus `js/quirk-objects.mjs`, `ObjectHost` for R4c's `ReadContext`, regex rows `r4b`, unit cases from the TS, M1 and no-panic tests; declarations in `uo-docx-d1a/R4b.md`) | Sonnet | one session (time not measured by the agent; plus short waits on other frontiers' transient build breakage); 0 verifier rounds (29,877 comparisons equal on the first complete replay after one bug the corpus found and one named difference), rework only on the corpus and the capture normalisation | ≈ 170 |
| R4c paragraphs, runs, fields and the custom-inline `JSON.parse` (`read/paragraph.rs` = `ReadContext`/`readParagraph`, `read/json_parse.rs`; facets `paragraph` (+ sources variants) and `custom-tag` with `js/capture-paragraph.mjs`, `js/capture-custom-tag.mjs`, generated corpus `js/quirk-paragraph.mjs`, regex rows `r4c`, unit, M1 and no-panic tests; declarations in `uo-docx-d1a/R4c.md`) | Sonnet | one session (time not measured by the agent); 0 verifier rounds (22,000+ facets equal after the corpus and the capture normalisation were corrected; one port bug found by the first replay: key marks of `ins`/`del`/`commentIds`), rework on the no-panic findings (comment-id memory, stack cost of nested stories) | ≈ 140 |
| R6 source store build (`read/sources.rs`, `SourcePart` no-copy change; facets `sources-para`, `sources-blocks`, `sources` (replay waits for R5), `js/capture-sources*.mjs`, synthetic package generator `js/quirk-sources.mjs`, regex rows `r6`, unit, M1 and no-panic tests; declarations in `uo-docx-d1a/R6.md`) | Sonnet | one session (time not measured by the agent; plus short waits on other frontiers' transient build breakage); 0 parity rounds (the interim facets matched on the first full replay), rework only on my own test expectations | ≈ 100 |

## Milestone-1 gate — approved 2026-11-03

Maintainer, verbatim: "Approve all" — every final recommendation of the milestone-1 questions accepted (G1–G27). Cross-record changes from the uo-render/uo-pdf plans (RQ1–RQ3, RQ13, PQ1, PQ8) apply to this record where listed there.

## Milestone 1 — D1a foundation

*(Opus, 2026-11-03, hybrid mode.)* The prerequisites of A7, so that the Sonnet frontiers R1–R6 port
the reader 1:1 inside fixed shapes. Crate `core/crates/uo-docx` (G1 features: `canon`, `session`,
`editing-api`, `doc`, `odf`, `export`, `compare`; the read path is always built), plus additions to
`uo-drawingml` (G12) and `uo-jsnum` (`json`). Nothing committed by this stage.

### A. What exists

| Piece | Where | Content |
|---|---|---|
| JS value plumbing | `src/js/` | `JsNumber` (`Num(f64)` \| `Null`), `Nullable<T>`, `NumKey` (SameValueZero `Map` key), `FieldMask` (cleared = explicit `undefined`), `KeyOrder` (insertion order), `JsRecord<V>` (plain-object dictionary in JS own-key order: integer-like keys first), `JsMap` (= `IndexMap`, TS `Map`), `JsonValue`, the `JsValue` trait (digest + `JSON.stringify`), the `js_object!` macro (`opt`/`req` fields under their TS names, `hidden {}` Rust-only fields, `custom_value:` for hand-written aliases; generates `JsObject` with compile-time sorted keys, `mark`/`mark_undefined`, the digest and the JSON) |
| Model (A2) | `src/model/` | ≈ 165 TS types field by field: `props` (ooxml/props.ts), `effects` (text-effects.ts), `drawing` + `color` (drawing.ts), `picture` (picture-xml.ts read types), `run`, `para` (document.ts), `table`, `section`, `story`, `document` (fulldoc.ts), `formatting` (parts.ts, styles.ts), `background`, `media` (`MediaStore`, dedupe by full bytes, G24), `sources` (`SourceStore` with `SrcText` spans into the parts' text) |
| Identity and text | `src/ids.rs`, `src/text.rs`, `src/pvec.rs` | `ParaId`/`TableId`/`DrawingId`/`HfPartId`/`MediaId`/`SectionBreakId`/`TxbxSrc` + `IdAlloc`; `SourceKey` (a value); `Utf16` (`Box<[u16]>`, lone surrogates kept, G6); `PVec<T>` (chunks of 64–128 `Arc<T>`, start index for O(log n) `get`, copy-on-write `make_mut`, `shared_chunks` for the U19 step cost) |
| Layout-facing API (A4, G21) | `src/view.rs` | `DocView` (body, sections, `hf_part`/`hf_story`, `hf_slots(section, Header \| Footer)` = the K13 slot → part map, notes, comments, bookmarks, formatting, styles, core props, background, `media`/`media_arc`, `formatting_version`, `story(StoryRef)`, `block`/`paragraph(StoryRef, BlockPath)`, `run(RunRef)`, `find_table`/`cell(CellKey)`/`find_textbox` (O(document) walks), `revision_authors` = `revisions.ts`, iterative); `DocRef`, `StoryRef`, `BlockPath` (inline up to two table levels, `Display` = `b3/r0/c1/b0`), `RunRef` + `RunSub` (field result, group item), `CellKey`, `NoteKind`, `HfWhich`; `LayoutOracle { field_values, next_hyphenation }`, `FieldScope`, `HyphenSkips` (`(ParaId, offset)`), `HyphenCandidate`, `LayoutError` + kind |
| Reader context (A5, G7, G20) | `src/read/ctx.rs`, `src/read/plain.rs`, `src/read/mod.rs`, `src/limits.rs`, `src/error.rs` | `ReadCtx` (limits, `IdAlloc`, `MediaStore`, per-read `wrapIds`/`blockSdtKeys`, `enter_nested`/`leave_nested` at `max_nesting` 64, per-part paragraph/run counts, `xml_part` with the docx presets, `rels`, `resolve_target` = uo-opc D6 only, `media_part`, `source_part`); `plain_paragraphs` (the G20 text fallback); the `DOCX`/`DOCX_POS` presets (`CANONICAL_PREFIX` table moved here from the uo-opc tests); `ReadOptions`; the `read_document` and `read_embedded_fonts` signatures (stubs until R5); `EmbeddedFont { data: Arc<[u8]> }`; `DocxLimits`, `DocxError`; one empty module per frontier file (§B), so that parallel frontiers never edit a shared Rust file |
| Canonical digest (A6) | `src/canon.rs` (feature `canon`), `js/canon.mjs`, `js/canon-selftest.mjs`, `tests/canon_selftest.rs` | the item encoding (in `canon.rs`), `CanonWriter` (media, source parts, relabel spaces, key-order facet), `Document::canon_full` / `canon_sources`; the TS walker with the alias rules (`@views`: `synced` \| `phantom` \| `split`), relabelling, `at` UTF-16 → UTF-8 by the part where `raw` sits. **Self-test: the TS walker over a hand-built TS document (aliased views, Maps, explicit `undefined`, NaN, `-0`, a lone surrogate, bytes, colour specs, custom paths, integer-like record keys, source spans, wrap ids) and the Rust `Canon` of the equivalent hand-built model give identical streams: full 726 items, key-order facet 384, sources 196** |
| Regex templates (G9) | `src/read/matchers.rs`, `js/regex-diff.mjs`, `js/regex-sites/templates.mjs`, `tests/regex_templates.rs` | `Cur` (literal, ASCII-`/i` literal, `\s*`, `\s+`, `\d+`, classes, `\b`, `$`) and the templates `is_one_of`, `find_word`, `find_lit_then_boundary`, `keep_ascii_digits`, `all_in`, `replace_char`, `newlines_to_space`, `self_closing_to_open`, `element_text` (dynamic from a constant), `find_attr_literal` (dynamic from data); the per-site template in the module doc. **12 template rows × 47 tricky inputs = 564 results, all equal to V8** |
| Data tables | `tools/gen-data.mjs`, `src/data/` (generated, committed, `#[rustfmt::skip]`), `tests/data_tables.rs` | `word-styles.ts` (blank parts, `WORD_BUILTIN_STYLES`), `style-names-es.ts` (`BUILTIN_STYLE_NAMES_ES` evaluated, JS order), `table-styles-builtin.ts` (`BUILTIN_TABLE_STYLES`); deterministic (two runs, 0 changed); headers carry the sources' SHA-256 (`data_sources_unchanged`); opt-in `data_regenerates_identically` (`UO_TS_PARITY=1`, `--check`) passes |
| JSON (LQ20) | `uo_jsnum::json` | `push_number` (`null` for NaN/±∞, `0` for `-0`, else `String(x)`), `push_string`, `push_string_utf16` (`QuoteJSONString`, lone surrogates as `\udxxx`); uo-docx `JsonWriter` + `JsValue::json` for every model type (insertion order through `KeyOrder`, `undefined` omitted, `Uint8Array` as V8 prints it, `Map`/`Set` → `{}`): the base of uo-docx-layout's `json_key`/`json_eq` |
| Colour + theme (G12) | `uo_drawingml::{dml, theme, CustPath, CustCmd}` | §C |
| Memory (M1) | `tests/memory.rs`, uo-drawingml `tests/memory_dml.rs` | every allocating operation of the foundation (values, `PVec`, model clone, view walks, digest, JSON, `ReadCtx` incl. error paths, `plain_paragraphs`, matchers; colour and theme readers): live bytes back to the start, 0 B |

Sizes: ≈ 7.5k lines of Rust in uo-docx (model ≈ 2.7k), ≈ 0.5k lines of JS tooling, 0.49 MB of
generated data; uo-drawingml +≈ 0.7k lines (colour, theme, custom paths, tests). Checks: `cargo fmt
--check`; `cargo clippy -p uo-docx -p uo-drawingml -p uo-jsnum --all-targets --all-features -- -D
warnings` (and for wasm32); the float-cast script and the grep bans on both crates; `cargo test -p
uo-docx -p uo-drawingml --all-features`; the wasm32 build (`--profile wasm`): all clean.

### Decisions taken beyond the architecture (for the milestone gate; none changes a result)

| # | Decision | Why |
|---|---|---|
| F1 | Every TS `number` of the model is `JsNumber` (`Option<JsNumber>` for `?:`), not a per-field choice | `Option<JsNumber>` is 16 bytes like `Option<f64>`; every NaN/±∞ and the post-undo `null` is representable without a declaration; the frontiers still declare the NaN sites (item 3) |
| F2 | TS string-literal unions are `Str`; a frontier narrows one only when its reader validates the value (item 9, Opus check) | the readers cast attributes with `as`: any string can be stored |
| F3 | `RunProps`'s required booleans are `Option` | `Partial<RunProps>` (resolved styles, patches) is the same type; the reader always sets them |
| F4 | Every model object carries `cleared: FieldMask` + `keys: KeyOrder` (24 B); field declaration order = the reader's fixed assignment order; data-dependent orders (`readRPr`, `readPPr`, `readSpPr` fills…) are recorded with `mark` after **every** assignment | K9/LQ20 need the insertion order; most objects never allocate a `KeyOrder` |
| F5 | TS unions of object literals (`FxFill`, `MathNode`, `PageFill`…) are permissive structs (every variant key optional, `type` a `Str`) | every combination the reader or an edit builds is representable; the digest is the TS object's |
| F6 | All ids are `u32` (`HfPartId` included) | no truncation decision |
| F7 | Notes are keyed by `NumKey`, not `i32` | `parseInt` of a note id can be NaN, a valid TS `Map` key |
| F8 | `Bookmarks` = `starts` (TS `bookmarks` + `bookmarkOffsets`: same keys, same order) + `ends` (its own order) | the TS keeps three maps; two orders are observable |
| F9 | A section's header/footer slots are `JsRecord<HfPartId>` (slot type → part); parts stored once per path (`hfCache`) | the slot type is any string the file wrote; one part serves several slots and sections (K13) |
| F10 | `TextboxView { Synced, Phantom, ParasOnly }`; the phantom empty paragraph of `textboxBlocks` is not stored | the TS `paras` of a textbox without paragraphs is a fresh object outside `blocks`; `ParasOnly` is the comments path (`readBlocks` absent) |
| F11 | Digest specifics: synthetic `@views` key (last in the order facet); the `SourceStore` facet has the store's maps only (`paraRefs`/`tableRefs`/`memo` out); `at` compared in UTF-8 (the walker finds the part where `raw` sits) | A6 left the encoding open; fixed in `canon.rs` and `canon.mjs` |
| F12 | Drawing colours are `DrawingColor(uo_drawingml::dml::WordSpec)` (the structured `srgbOf` output); digest and JSON write the TS spec string back. The picture-xml colour (`{kind, val, lastClr?, mods?}`, parsed numbers) stays its own type `PicColor` | G12 structured colours; the two TS colour shapes are two different values |
| F13 | `CustPath` numbers are `f64` (uo-drawingml type, shared with uo-render); D2a normalisation maps NaN to 0.0 there | after an undo the TS holds `null`, which every use of these numbers (arithmetic) reads as 0 |
| F14 | `SourceStore` raw strings are `SrcText` (a span into the part text the XML reader parsed, or owned when the TS builds the string); the store keeps the parts' text (`Arc<str>`) | A2 spans; the digest and the writer need the text |
| F15 | `PVec` is public (`Story` exposes it); `uo_docx::history` (G5) builds on it | the layout reads stories |
| F16 | `json` always compiled, `canon` behind the feature | the layout's JSON keys run in production; the digest is tooling |
| F17 | Recursion across stories (cell → table → cell, textbox content) may stay recursive, bounded by `enter_nested` (64); walks inside a story are iterative | item 5 is "made iterative **or limited**"; the wasm stack test at the limit (G20) comes with R5 |
| F18 | G20 fallback = every `w:p` below the element, in order, as one run of its text (`w:t`, TAB, LF for `w:br`/`w:cr`, U+2011, U+00AD) with `DEFAULT_RUN_PROPS` | U18 asked to state the outcome |
| F19 | `LayoutOracle::next_hyphenation(view, &DocRef, &HyphenSkips) -> Result<Option<HyphenCandidate>, LayoutError>`, `HyphenCandidate { at }` = the line's `hyph` position (body only, as `nextManualHyphenation`) | the approved shape had no error path; layout errors are values (LQ2) |
| F20 | `RunRef.sub` addresses one nesting level (`FieldResult(i)`, `GroupItem(i)`); deeper is an R4b declaration if it occurs | K1 reach without a path type per run |
| F21 | New `DocxLimits`: `max_paragraphs_per_part` 4 M, `max_runs_per_part` 16 M, `max_numbering_levels` 9, `max_based_on_chain` 10 k (with `max_fields_per_story` 100 k and `max_field_depth` 64 from C#) | proposals for the milestone security review (a limit is an error, not a different result) |
| F22 | `Document.styles` (TS `FullDocument.styles`) is top-level, next to `formatting` | the TS shape (A2 sketched it inside `DocFormatting`) |
| F23 | The data generator imports the TS **sources** of the three self-contained data modules (Node 24 type stripping), not `dist/` | `style-names-es.js` is missing from the built `dist/` of main; the sources are the frozen truth; `TABLE_FIXED`/`TABLE_FAMILY` are code → R2 |
| F24 | Theme: `uo_drawingml::theme::read` gives an app-neutral as-written view (`ThemePart`); Word's `readTheme` rules live in uo-docx (R2) | the same part serves Slides/Sheet with other rules; `readSpPr` is Word's |
| F25 | uo-drawingml depends on uo-opc (the `Element` of `read_color`/`theme::read`) | G12 `color::read(&Element)` |
| F26 | `srgbOf`'s `toUpperCase` uses Rust `to_uppercase` | identical on ASCII (every real colour value); a non-ASCII hex string could differ by Unicode version — listed, not measured |
| F27 | Named difference (G12): the TS splits the spec string on `\|`, `,` and `=` when it resolves; a raw modifier value containing one of them resolves differently in Rust (the structure read from the XML) | accepted by name; R1's quirk corpus counts it |

### B. Brief for the Sonnet reader frontiers R1–R6

**Common to every frontier** (give this section, A2, A5, A7 and the TS files to the agent):

- Worktree `rust-core`, no commit/push/stash, LF, `CARGO_TARGET_DIR=core/target/docx-<rN>`. Edit only
  your files: your `src/read/<module>.rs` (already declared in `src/read/mod.rs`), your tests
  (`tests/parity_<facet>.rs`, `tests/regex_<rN>.rs`, `tests/memory_<rN>.rs`), your scripts
  (`js/capture-<facet>.mjs`, `js/regex-sites/<rN>.mjs`) and your declaration file
  `core/docs/modules/uo-docx-d1a/<RN>.md` (pre-created with the nine sections). Never change a model
  type: a needed change is an item-9 row and a stop for the Opus check.
- Port 1:1 (same algorithm, same order of floating-point operations, same results). TS is the
  behaviour source; C# `DocumentReader.cs`/`FullDoc.cs`/`Ooxml/*` only for types.
- XML: `uo_opc::xml::Element` — `firstChild(el, n)` → `el.first_child(n)`, `childElements(el, n)` →
  `el.children_named(n)`, `childElements(el)` → `el.child_elements()`, `el.attrs['x']` →
  `el.attr("x")`, `el.s/cs/ce/e` → `el.positions()` (UTF-8), `el.raw` → `el.raw()`, `textContent` →
  `text_content()`, `findDeep` → `descend`; `Object.entries(el.attrs)` is JS object order
  (`uo_jsnum::object::js_object_order`). Parts through `ReadCtx::xml_part` (positions when reading
  with sources) or `xml_part_plain`, relationships through `rels` + `resolve_target` (D6 only, no
  `word/${target}` rules), media through `media_part`.
- Numbers through uo-jsnum: `parseInt` → `parse::parse_int` (`parse_int_utf16` over units),
  `Number()` → `parse::to_number`, `parseFloat` → `parse::parse_float`, `Math.*` → `math::*`,
  `x || 0` → `conv::truthy`, `x | 0` → `conv::to_int32`, `String(n)` → `format::to_js_string`. Text:
  `trim` → `text::trim`, `split(/\s+/)` → `text::split_js_space`, `toLowerCase` →
  `text::to_lowercase`, UTF-16 lengths/slices → `text::utf16_len`/`utf16_slice`.
- Regex sites → `read::matchers` (the per-site template in its module doc) + one row per site in
  `js/regex-sites/<rN>.mjs` (`SITES`, optional `INPUTS`) and a Rust test `tests/regex_<rN>.rs` over
  `tests/regex/<rN>.expected` / `<rN>.inputs.txt` (pattern: `tests/regex_templates.rs`); regenerate
  with `node core/crates/uo-docx/js/regex-diff.mjs`.
- Model writes: ids from `ctx.ids` (`para()`, `table()`, `drawing()`) when the TS creates the
  object, in document order; `obj.mark("k")` after every assignment of a data-dependent order
  (declared in item 4); `obj.mark_undefined("k")` for a key the TS writes as `undefined`
  (`{ widthTwips: x ? … : undefined }`); `Arc::new` for `props`/`ppr`/`rpr`; colours through
  `uo_drawingml::dml::word_spec` into `DrawingColor`.
- Process-global TS counters: `ctx.next_wrap_id()`, `ctx.next_block_sdt_key()`. Nested stories:
  `if ctx.enter_nested() { …; ctx.leave_nested() } else { plain::plain_paragraphs(el, &mut ctx.ids) }`.
  Per part: `ctx.begin_part(name)`, `count_paragraph()`, `count_run()`.
- No panic, no `unwrap`/`expect`/indexing that can panic on document data; a `try {…} catch {}`
  keeps its partial result as a value (item 7); float → int only through `uo_jsnum::conv`.
- Done = `cargo fmt --check`; `cargo clippy -p uo-docx --all-targets --all-features -- -D warnings`
  (also `--target wasm32-unknown-unknown`); `bash core/tests/float-casts.sh` and
  `bash core/tests/grep-bans.sh` clean for uo-docx; `cargo test -p uo-docx --all-features` green
  (canon self-test, regex, data, memory, your parity replay at 0 mismatches or only named
  acceptances); an M1 test per public function incl. error paths; the nine declaration sections
  filled; wall time, rounds and rework in your declaration file's process metrics.

**The nine declarations** (A7; one table per kind in `uo-docx-d1a/<RN>.md`, each row with the TS
file:line): (1) numeric parse/rounding sites and the uo-jsnum function used; (2) regex sites and
their matcher, the dynamic ones named; (3) fields that can store NaN/±∞ and keys stored as
`undefined`; (4) `Map`/object orders relied on and key orders recorded; (5) recursions and how they
are made iterative or bounded; (6) module-level/process-global state and its per-read replacement;
(7) `try/catch` sites and the partial result kept; (8) aliasing (two model positions sharing one TS
object) and how A2 gives the same observable result; (9) saturating/wrapping/truncating choices and
data-shape deviations from A2 (Opus check first).

**Parity** (Node 24.15 x64 only, G17; captures hold digests only):
`UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-<facet>.mjs` writes
`core/crates/uo-docx/tests/parity/<facet>.capture` (uo-testkit `CaptureWriter`, `@platform`, keys
`<docx path relative to the corpus>#<part>#<ordinal>`), each value the hex digest of the `Canon`
walk (`js/canon.mjs`) of the TS function's output for that input; `tests/parity_<facet>.rs`
replays with `uo_testkit::{Capture, Replay}` (`assert_clean`; `Replay::accept` only for differences
accepted by name in this record). Corpus: `write/packages/docx-layout/test/oracle/**/*.docx` (2,742),
the docx-core test fixtures, and the generated attribute-quirk corpus (G18: R1 writes
`js/quirk-corpus.mjs`, a seeded generator of docx parts with `"1e3"`, `" 12 "`, `"12pt"`, `"0x10"`,
`"-0"`, `"NaN"`, empty and missing attributes, integer-like keys, duplicate ids, nesting past 64,
`basedOn` cycles and 10 k chains, non-BMP text and surrogate character references; the others
extend it). Traces: `UO_PARITY_TRACE=<key>` on both sides (traced `Stream`s), as
`tests/canon_selftest.rs` does with `UO_CANON_TRACE`.

| Frontier | Rust files (`src/read/`) | TS source | Facet (input → digest of) | Notes |
|---|---|---|---|---|
| **R1** | `props.rs`, `text_effects.rs` | `ooxml/props.ts` (all readers, `int`, `measureTwips`, `onOffVal`), `text-effects.ts` read side (`readColor`, `readFill`, `readTextEffects`, `elementXml`) | `props`: every `w:rPr`, `w:pPr`, `w:tblPr`, `w:tblPrEx`, `w:trPr`, `w:tcPr` of document, styles, numbering, headers, footers, notes → the read object | `readRPr`/`readPPr`/`readTblPr`/`readTrPr`/`readTcPr` orders are data-dependent: `mark` every key; `measureTwips` is a `Cur` walk; also writes `js/quirk-corpus.mjs` |
| **R2** | `parts.rs`, `styles.rs`, `table_styles.rs`, `style_names_es.rs` | `ooxml/parts.ts`, `styles.ts`, `table-styles.ts` read side, `style-names-es.ts` (the code; the tables are `data::`) | `parts`: `DocFormatting` per docx; `styles`: `StylesMap` per docx | theme = Word rules over `uo_drawingml::theme::read` (the mapping in uo-drawingml `tests/theme_parity.rs` is the reference to lift); `fillStyles`/`bgFillStyles`/`effectStyles` call R3's `drawing::read_sp_pr(sp_pr: Option<Element<'_>>, style_el: Option<Element<'_>>) -> ShapeStyle` (signature fixed here); `partByRelType` → D6; `basedOn` iterative with a seen set and `max_based_on_chain` |
| **R3** | `drawing.rs`, `vml_shape.rs`, `page_background.rs`, `watermark.rs` | `drawing.ts` (all), `vml-shape.ts`, `page-background.ts` `readPageBackground`, `watermark.ts` `isWatermarkXml`/`readWatermarkXml` | `drawing`: every `wp:anchor`/`wp:inline` (anchor, spPr style), every VML shape style, the page background | `srgbOf` = `uo_drawingml::dml::word_spec` (validated on 1,500 cases); `readCustGeom` with uo-jsnum fdlibm trig into `uo_drawingml::CustPath`; `WatermarkInfo` is a result type R3 defines in its file (not part of `FullDocument`) |
| **R4a** | `props_map.rs` | `document.ts`: `readBorderEdge`, `runPropsFromRPr`, `themeRefOf`, `revInfo`, `editBaseProps`, `paraPropsFromPPr`, `readSectPr` | `props-map`: every rPr/pPr/sectPr of the corpus → the editing props | `RunProps`/`ParaProps` are declared in these functions' assignment order; `fontAscii: rpr.rFonts.ascii` can write `undefined` (`mark_undefined`) |
| **R4b** | `inline_objects.rs`, `picture_format.rs` | `document.ts` lines 185–1583 (images, textboxes, shapes, groups, SmartArt, VML runs, `sync*FromXml`, `blipFxImageEffects`); `picture-xml.ts` lines 100–556 (read side) | `drawing-runs`: every `w:drawing`/`w:pict`/`w:object`/`mc:AlternateContent` run → the `Run` | after R4a; textbox content through R5's block reader (`fulldoc::read_blocks_of`, grouped) inside `enter_nested`; `TextboxView` per F10; `rawSrc` (module state of picture-xml.ts) becomes a parameter |
| **R4c** | `paragraph.rs`, `json_parse.rs` | `document.ts` lines 1583–2213 (`ReadContext` as a struct borrowing `ReadCtx`, `readRunsInto`, fields and nested field codes, custom inlines, math, `readSingleRun`, `readParagraph`, `collectInstrText`, `buildReadContext`); `JSON.parse` of the custom-inline tag | `paragraph`: every `w:p` of every story → the `Paragraph` | after R4a; callbacks (`onBookmark`, `resolveImage`, `readBlocks`…) become methods or returned events, never stored closures; the field stack is bounded by `max_field_depth`; `captureParagraph` is R6's (call it) |
| **R5** | `fulldoc.rs`, `inspect.rs`, `blank.rs`, `fonts.rs` | `fulldoc.ts` (read side: tables, `unwrapRaw`, blocks, header/footer parts, notes, comments, sections, `linkVmlTextboxes`, `readFullDocument`, `readCoreProps`, `readSectionProps`, `readBodyFragment`), `inspect.ts`, `blank.ts`, `fonts.ts` | `full`: `Document::canon_full` per docx (sources off and on) | last (after R2, R3, R4c); `read_document`/`read_embedded_fonts` replace the stubs; `markSectionBreaks` is the session's (not ported here, A5); the wasm test at depth 65 (G20) |
| **R6** | `sources.rs` | `source.ts` reader side (`id`, `addPara`/`addTable`/`addRow`/`addCell`/`addTextboxGroups`), `document.ts` `captureParagraph`, the source parts of fulldoc's `storyOf`/`groupOf`/`wrapOf` | `sources`: `Document::canon_sources` per docx | starts with R4c (shared `ReadCtx::sources`), finishes after R5; spans via `ReadCtx::source_part` + `positions()`; `openOf` = `matchers::self_closing_to_open` (owned `SrcText`); signatures excluded |

**Order:** R1, R2, R3 and R4a in parallel; R4b and R4c after R4a; R6 with R4c; R5 last (R6
finishes with it). The `parity-verifier` and `memory-verifier` (Sonnet) run on each frontier as it
lands; the orchestrator folds the eight declaration files into this record at the milestone gate.

### C. Colour and theme API (G12) — status

**Implemented 1:1, with TS differentials** (uo-drawingml):
- `dml::DmlColor` / `ColorMod` / `ColorKind` (as-written colour elements: name, `val`, `lastClr`,
  every child; parsed at use), `read_color(el, attr_prefix)` (`""` for `a:`, `"w14:"` for Word's
  text effects), `first_child_by_priority`, `first_child_in`, `SchemeColors` (+ impls for slices and
  uo-docx's `JsRecord<Str>`), `NoScheme`.
- The Word variant: `word_spec(parent, theme)` = `srgbOf` (priority srgb → scheme → sys → prst,
  read-time theme lookup, `@key` references, alpha, raw modifiers), `WordSpec::to_ts_string` (the
  TS spec string), `WordSpec::parsed_mods`, `resolve_word_spec` = `resolveDrawingColor`
  (`THEME_ALIAS`, six-digit check, `apply_color_mods`, `toHex` including its `NAN` print),
  `word_theme_slot`, `is_hex6`, `hex6_rgb`, `to_hex_upper`. **Differential
  (`tools/capture-dml-color.mjs`): 1,500 seeded `a:spPr` fills through the frozen `readSpPr` and
  `resolveDrawingColor` (every kind, priority pairs, invalid hex, missing/empty attributes, 19
  modifier names with numeric, empty, `1e3`, `abc`, ` 5000` and fractional values): 1,500/1,500
  equal** (spec string, alpha, resolved colour).
- `theme::read(&Document) -> ThemePart` (font sets, scheme entries, line widths, style-list element
  ids; `find` = `descend`). **Differential (`tools/capture-theme.mjs`): the distinct theme parts of
  the oracle corpus (2) and 300 seeded synthetic themes through the frozen `readTheme`; the Word
  rules applied to the view rebuild the TS theme exactly on all 302** (`tests/theme_parity.rs`).
- `CustPath`/`CustCmd` (the custGeom value shared by the model and uo-render).

**Stub list** (each with its first consumer; none is on milestone 1's read path):
1. charts `dmlColor` (`shared/charts/src/dml-color.ts`: `#`-prefixed result, its own preset table and
   modifier walk) — uo-charts;
2. the Slides resolver (`slides-core/src/theme.ts`: `clrMap`, `phClr` substitution) — the Slides port;
3. picture-xml `dmlColorSpec` (numeric modifiers into `resolve_word_spec`) — R4b/D2b, a few lines
   over the existing API;
4. the w14 `FxColor` resolution of text effects (layout `textfx.ts`) — uo-docx-layout, over
   `apply_color_mods`.

### D. Not done here (by design, or left)

- The reader itself (R1–R6), its parity corpus and captures, the attribute-quirk generator (R1), the
  G20 wasm stack test (R5), the D1a M2 soak and M3 budgets (memory-verifier once R5 lands).
- `uo_docx::word` (LQ3) is an empty module for uo-docx-layout F2; `JSON.parse` for custom inlines is
  R4c's; the session types of A3 come with milestone 2.
- The CI workflow is unchanged: uo-docx joins `cargo test --workspace --all-features`, clippy and the
  ban scripts by being a workspace member; the Node scripts (captures, canon self-test, regex
  reference, data generator) are manual steps whose outputs are committed.

### E. Integration fixes (after R1–R3, before the milestone gate)

Three small changes decided by the orchestrator while integrating the frontiers; the first two close
declarations of R1 and R2, the third a defect of the foundation walker.

| # | Change | Result |
|---|---|---|
| I1 | **R1 item 9, row 1 (A1)**: `TblBorders` (model, `model/props.rs`) gets `tl2br` and `tr2bl` (`opt`, after `insideV`, the order `readBorders` assigns them in); `read::props::tbl_borders_of` copies them from `borders_of` like the TS (`p.tblBorders = readBorders(el)`). The capture normalisation and the `@accepted A1-tblBorders-diagonals=51` line are gone from `js/capture-props.mjs`; `props.capture` regenerated (Node 24.15 x64) | `parity_props`: 28,467 facets, 0 mismatches, **0 accepted for A1** (the 2 accepted are A2 and A3, unchanged); `props_parity_table_diagonals_are_kept` pins the three keys. Every other uo-docx test green (`TblBorders` is shared with `table_styles`/`props_map`) |
| I2 | **R2 items R2-4/R2-5**: `DocxLimitKind::{NumberingLevels, BasedOnChain}` added (`error.rs`); the two `DocxError::Corrupt` sites for `max_numbering_levels` (`parts::read_numbering`) and `max_based_on_chain` (`styles::read_styles_xml`) are now `DocxError::Limit { kind, max, part }`, the part name filled in by `DocxError::in_part` at the two `ReadCtx` call sites (`read_doc_formatting`, `read_styles`; the `*_text` helpers leave it `None`). **Limit values unchanged**: the 9-level numbering limit (the TS reads every level; the oracle corpus has none above) stays on the milestone security review's list | `r2_unit` (22 tests) and `parity_parts` (the `R2-limit` acceptance now matches on the `Limit` kinds; 5 accepted as before) updated |
| I3 | **Canon walker**: `js/canon.mjs` looks the context rules up with `Object.hasOwn(RULES, key) ? RULES[key] : undefined` (a dictionary key such as `constructor` found `Object` and broke the destructuring). R2's workaround in `js/capture-r2-common.mjs` (the `object` override that renamed `Object.prototype` keys) is removed; `R2Canon` stays for its lone-surrogate `str` | canon self-test output identical; `parts`, `styles`, `table-styles`, `props-map`, `drawing` captures regenerated **byte-identical** (`props` changes only by I1). `capture-word.mjs` (F2's) was not rerun |

### E. `word` (uo-docx-layout frontier F2, LQ3) and its addition to the D1a foundation

`src/word/` holds the Word semantics shared by the layout and the session (record
`uo-docx-layout.md`, "Wave 1 — F2"). Two additions outside it, both additive: the `js_object!` macro
also implements `js::JsAssign` (`assign_slot`/`remove_slot`: typed per-key copy and removal, the base
of `word::obj`'s `{ ...a, ...b }`, `Object.assign` and `delete` with the key insertion order, K9/LQ20),
and the crate depends on uo-text (UTF-16 case mapping and JS white space over units).

## Stop point (2026-11-03, maintainer asked to stop after this round)

Done in D1a: foundation (Opus), R1, R2, R3, R4a, R4b, R4c, R6 (Sonnet) and the integration fixes
I1–I3; every frontier's facets equal to the frozen TS (accepted only by name). Parity rounds:
R4b 1 (found by its own first replay: `linkedTxbx seq` is `|| 1`), all others 0.

Next: **R5** (fulldoc/inspect/blank/fonts — the whole-document read; it unlocks the `sources`,
`full` and `hyphlangs` facets and the text box `Blocks`/`Grouped` modes R4b leaves as `Err`).

Open, for the next round or the milestone-1 review:
- **Nested-story stack** (R4c): a text box inside a text box costs ~14 KB of native stack per level
  in release (~50 KB debug), mostly R4b's VML/drawing readers; 64 levels do not fit the 1 MiB wasm
  stack. Slim R4b's frames (or make the story recursion iterative) before R5's G20 wasm test.
- **Open comment ranges** (R4c item 9.2): >64 concurrently open ranges is `Corrupt` today; the TS has
  the same O(n²) memory. Proposal: a `DocxLimits` field and a `DocxLimitKind` variant (security review).
- **`MathNode.rpr`** cloned per node (R4c 9.6): proposal `Arc<RPr>` in the model.
- **Legacy flat `readDocument`** (R4c 9.7) not ported: confirm no consumer remains.
- **9-level numbering limit** (R2), unbounded `Math.min` over wrap polygons (R3), the R4b item 9 rows:
  milestone security review.
