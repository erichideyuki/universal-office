# uo-fonts — font files, OpenType kerning and ligatures, embedding, hyphenation

Record of the rust-port workflow. Status: **gate 1 approved 2026-11-01** (G1–G16 as the final recommendations); stage 3 phase 1 (foundation: skeleton, `error`, `limits`, `js`, `read`, `sfnt`, `generate::sfnt`) done, phase 2 (frontiers B, C, D) next.

Dependencies on earlier gates: `uo-opc` passed gate 2 (D1–D8); `uo-testkit` and `uo-alloc` are
in place. *(revised after the skeptic, S15)* uo-fonts does **not** wait for `uo-jsnum`: the JS
helpers it needs live in a private `js` module and move to uo-jsnum after its gate 2 (G8).

## Scope

### TS covered (behaviour source, frozen on `main` 0.9.x)

| TS file | What moves to uo-fonts |
|---|---|
| `write/packages/docx-layout/src/typography/font/sfnt.ts` | `SfntFont`: table directory, `FontMetrics` (hhea, OS/2 with the optional fields, post), cmap formats 4 and 12 with the (3,0) symbol fallback, hmtx, VDMX, COLR v0 + CPAL, `glyf` outlines (simple and composite), legacy `kern` fallback, GPOS cache per bytes |
| `.../typography/font/gpos.ts` | `parseGposKern` (PairPos formats 1 and 2, extension lookups, class 0), `gposKernInLatin`, `kernKey` |
| `.../typography/font/gsub.ts` | `parseGsubLigatures` (LigatureSubst format 1, SingleSubst formats 1/2, extension), `applyLigatures` |
| `.../typography/font/layout-font.ts` | the `LayoutFont` contract, now the methods of `Face` |
| `.../typography/hyphen/liang.ts`, `hyphen/index.ts` | Liang hyphenator, pattern ids, registry (without the dynamic `import()`) |
| `.../typography/hyphen/patterns/*.ts` (7 languages) | data, regenerated as data files (G7) |
| `.../src/fonts.ts` `isSymbolFont` | `Face::is_symbol_font` (a property of the font file, used by layout and PDF) |
| `shared/fonts/src/sfnt.ts` | `isFontFile`, `isEmbeddable`, `hasTrueTypeOutlines`, `glyphIdsFor` (its own cmap reader), `faceInfo` + name strings, `standaloneFace` (.ttc) |
| `shared/fonts/src/subset.ts` (+ the stale copy `converter/.../pdf-writer/subset.ts`) | `subsetTrueType` (gid-preserving, `keepLayout`) |
| `shared/fonts/src/eot.ts` | `sfntToEot` (EOT 2.2 without compression) |
| `shared/fonts/src/embedding.ts` | `planFontEmbedding` (two phases, app-neutral policy — revised after the skeptic, R-EMBED), `DEFAULT_EMBEDDING`, `COMMON_FONTS` |
| `shared/fonts/src/ooxml.ts` | `obfuscateOdttf` only; *(revised after the skeptic, S17, G2)* the read/write of the embedding settings and parts is deferred to uo-docx/uo-pptx |
| `shared/fonts/src/catalog.ts`, `types.ts` | `DEFAULT_FAMILY`, `BUNDLED`, `METRIC_SUBSTITUTES`, `PROBE_FAMILIES`, `bundledFileName`; `FontVariant`, `variantOf`, `variantOfStyle`, `isBold/ItalicVariant` |
| `write/packages/docx-core/src/fonts.ts` `deobfuscateFont` | `odttf::deobfuscate` (one module with the obfuscation) |
| `converter/packages/converter/src/{pptx,xlsx}/sfnt.ts` (identical copies) | `TtfProgram` → `FontProgram` (the converter's PDF font program: its own cmap preference, bbox, italic angle) |

### C# covered (type guide; behaviour the TS does not have moves too)

- `UniversalOffice.Core/Fonts/{SfntFont,Gpos,Gsub,LayoutFont,JsDataView,JsCompat}.cs`,
  `Hyphen/{Hyphenator,Hyphenation}.cs`: type guide only (ports of the TS).
- `UniversalOffice.Core/Pdf/TtfProgram.cs` (and its copy `Converter/Pdf/TtfProgram.cs`): the
  converter program **plus a C#-only legacy `kern` reader** (last pair wins, all-or-nothing on a
  malformed table, read whatever GPOS has) — moves to `FontProgram::legacy_kern` (G6).
- `Core/Pdf/SubsetTrueType.cs` (two copies): same as the TS subset.
- `UniversalOffice.Docx/EmbeddedFonts.cs` `DeobfuscateFont`: same as docx-core.
- Finding in the C#, not a parity target: `Gpos.JsMapMaxSize = 1 << 23` ("measured") is wrong
  for Node 24.15: a V8 `Map` (and `Set`) holds **16,777,216 = 2^24** entries; the insertion of
  the next one throws `RangeError: Map maximum size exceeded` (measured here, 2026-11-01, numeric
  keys above the Smi range). The C# drops GPOS kerning for fonts between 2^23 and 2^24 pairs, the
  TS keeps it. Rust follows the TS (G5).

### Left for other modules

| Part | Goes to | Why |
|---|---|---|
| `docx-layout/src/fonts.ts`: `FaceResolver` (Word substitution chain: `ALIASES`, `CJK_DEFAULT`, `genericFor`, Cambria/Calibri rule, `standIn`), `faceMetrics` (DirectWrite vertical metrics + the East Asian 1.3× rule), `VERT_TABLE`/`superscriptRaiseFor`/`subscriptDropPt`, `parsePostScriptName`, `withEmbeddedFonts`, `FontFace`/`FontProvider` | `uo-docx-layout` | Word rules measured on the Word oracle; neither Sheet nor Slides use them today. Reuse note: the Windows FontSubstitutes part of `ALIASES` and `parsePostScriptName` are app-neutral and can move to `uo_fonts::catalog` when Slides layout is ported. |
| The 600-DPI measuring model: `deviceEmPt`, `SfntFont.advanceTwips`/`kernTwips`/`kernGlyphsTwips` scaling, `setAdvanceTable` (empirical Word advances), `lineheight.ts`, `linebreak.ts`, `richbreak.ts`, `paginate.ts` | `uo-docx-layout` (`measure` module) | Word/DirectWrite rules (reuse specialist: keep them out of the core font API). uo-fonts exposes the font-unit primitives they are built on (`glyph_id`, `advance_width`, `kern_units`). |
| `docx-layout/src/hyphenate.ts` (Word hyphenation choice, NFC per cluster) | `uo-docx-layout` | Word rules; it calls `Hyphenator::hyphenate`. |
| `pdf-writer/font.ts` `EmbeddedFont` (Type0/CIDFontType2, `/W`, ToUnicode, the Symbol/Wingdings ToUnicode tables), C# `Core/Pdf/EmbeddedFont.cs`, converter `pdf-writer/font.ts` | `uo-pdf` | PDF objects; it calls `uo_fonts::subset_true_type` and `Face`/`FontProgram`. |
| docx-core `readEmbeddedFonts` (fontTable + rels), slides-core `fonts.ts` `readEmbeddedFonts` | `uo-docx`, `uo-pptx` (Slides) | Format readers; they call `odttf::deobfuscate`, `eot::to_sfnt` (later), `face_info`. |
| *(revised after the skeptic, S14)* `docx-layout/src/math.ts` `parseFont` (a fourth directory reader: last-wins `Map` keyed by `b[p]` indexing, only `head` required, `upm \|\| 2048`; MATH constants, italic correction by coverage, glyph ink box with its own `glyf + s + 10 > byteLength` check; `WeakMap` cache) | `uo-docx-layout` now (G15) | used only by the Word equation layout; moves to `Face::math()` in uo-fonts when Slides equations need it. It keeps its own reader rules (named in the uo-docx-layout plan), built on `uo_fonts::read::View`. |
| *(revised after the skeptic, S14)* slides-core `fonts.ts` `sfntStyle` (own OS/2 reader: weight in 1..=1000 else 400, `o + 64 > len` → null) and `looksLikeSfnt` (4 signatures, ≥ 4 bytes, no WOFF) | `uo-pptx` (Slides) | Slides' own rules over `uo_fonts::read::View`; the differences with `face_info`/`is_font_file` are named in that plan. |
| *(revised after the skeptic, S14)* the third `deobfuscateFont` copy (slides-core `fonts.ts`) and the tool `sheet/tools/excel-lab/deobfuscate-odttf.mjs` | replaced by `odttf::deobfuscate` (same rule as docx-core) | one copy |
| *(revised after the skeptic, S17, G2)* `ooxml.ts` read/write of the embedding settings and parts (`readDocxEmbedding`, `writeDocxEmbedding`, `readPptxEmbedding`, `writePptxEmbedding`) | **deferred to the uo-docx and uo-pptx plans** (recommended) | `word/fontTable.xml` and `word/settings.xml` belong to the uo-docx reader and lossless writer; the regex spec table below ("ooxml.ts") is handed to those plans. uo-fonts keeps the app-neutral pieces (plan, odttf, EOT, subset). |

### Deferred (module slots reserved in uo-fonts, not part of this gate 2 — G3)

- `eot::read`: EOT/MicroType Express reader (`slides-core/src/eot.ts`, 542 lines, LZCOMP + CTF;
  TS only) — ported with Slides; one copy for every app.
- `system`: the native system-font index (C# `SystemFonts.cs`, port of
  `write/tools/corpus-lab/node-fonts.mjs`; feature `system`, never in wasm) — ported with the .NET
  layout pipeline (`uo-ffi`).
- `symbols`: name-based symbol-font detection and the dingbat → Unicode table
  (`slides-core/src/symbol-fonts.ts`) — candidate for PDF export of every app, ported with Slides.

### Stays in the TS shell (browser APIs, not engine)

`shared/fonts/src/resolver.ts` (`FontResolver`: host, Local Font Access, `document.fonts`,
fetch), `environment.ts`, `faceDescriptors`, `node/system-fonts.cjs` (Electron main),
`shared/ui/src/font-embedding-dialog.ts`. They feed the Rust core with font bytes and read the
catalog from it (G9).

## Plan (planner, revised after the skeptic; gate 1 pending)

### Crate placement and reuse

One crate `uo-fonts`: everything above is about font files and is the same for Write, Sheet and
Slides; nothing in its API assumes Word.

| Part | Write | Slides | Sheet | Converter / .NET |
|---|---|---|---|---|
| `Face` (metrics, cmap, advances, kerning, ligatures, outlines, COLR, VDMX) | layout, PDF, WordArt outlines | text layout when it moves from canvas measuring | — (Excel width tables are Sheet's own) | DocxPdf |
| `FontProgram` | — | PDF export | PDF export | converter xlsx/pptx/docx PDF |
| `subset_true_type`, `standalone_face`, `face_info`, `is_embeddable` | PDF, docx embedding | pptx embedding, PDF | PDF | PDF |
| `odttf`, `eot::write` (and later `eot::read`) | docx | pptx | — | — |
| `embed` (plan; app-neutral policy, revised after the skeptic S19); the writers go to uo-docx/uo-pptx (G2) | docx | pptx | — (xlsx has no embedded fonts) | — |
| `hyphen` | layout | (none today) | — | — |
| `catalog`, `variant` | provider, viewer | resolver | resolver | substitute provider |

Existing crates reused:
- `uo-opc`: *(revised after the skeptic, S17)* **no dependency** once module 18 is deferred (G2).
  For the uo-docx/uo-pptx plans that take the writers: `Package::{part_names, part_text,
  set_part, set_part_text, remove_part, resolve_target}`, `rels::{rels_part_for,
  parse_rels/from_document, add_rel (Insert::BeforeLastCloseTrimmed — written for this code),
  remove_rels, first_free_rel_id}`, `content_types::{add_default (Insert::AfterRootOpen),
  add_override (BeforeLastCloseTrimmed)}`, `xml::raw::expand_self_closing`; the regex `.rels`
  reader of `ooxml.ts` is replaced by the shared parser (differences on malformed `.rels`
  accepted by name). The JS whitespace set of the private `js` module is the one of
  `uo_opc::xml::chars` (copied, the same table; uo-opc's is `pub(crate)`).
- `uo-testkit`: capture/replay, `Stream`, `Rng`; **new** `generate::sfnt` (a table assembler:
  tags + bytes → sfnt with a directory, any offsets/lengths/duplicates/checksums), dependency-free
  like `build_zip`, reused later by uo-pdf tests.
- `uo-alloc`: M1–M5.
- `uo-jsnum`: *(revised after the skeptic, S15, G8)* **not a dependency of this port.** uo-fonts
  has a `pub(crate) mod js` (spec in "Revisions after the skeptic", R-JS) that moves to uo-jsnum
  once uo-jsnum passes gate 2 and offers the same functions (including code-unit `parse_int` and
  the lowercase rules). uo-fonts uses **no transcendental, no `pow`, no `Math.round`** once the
  measuring model is in layout (see "uo-jsnum"). The uo-fonts line of `uo-jsnum.md` must be
  corrected to: `parse_int` over UTF-16 code units (radix 16), `to_uint8`, JS whitespace/trim,
  `lower_cp`, `js_to_lowercase` — nothing else.

### Crate vs own port

| Candidate | License | Verdict | Why |
|---|---|---|---|
| ttf-parser 0.25 | MIT/Apache | no | unmaintained (RUSTSEC-2026-0192); bounds per table (the TS reads through one view over the whole file: a short table reads the next table's bytes), tries every cmap subtable (TS: first-best only, a best-scored format-14 subtable rejects the font), no U+F000 fallback, hmtx `None` past `numGlyphs` (TS: last advance), OS/2 optional (TS: required) |
| read-fonts / skrifa | MIT/Apache | no | +251 KB raw / +64 KB brotli; f32 outlines, FreeType contour start (TS: first on-curve point or the midpoint of two off-curve points, f64 composite transforms, depth 8, 64 components) |
| rustybuzz / harfrust | MIT | no | +685–1,019 KB raw; real OpenType shaping ≠ the Word emulation (union of `kern` features of every script, first value wins, first glyph XAdvance only, lookup flags ignored, own class-0 rule, legacy fallback when GPOS yields 0 pairs; ligatures: requested liga/clig/dlig/hlig only, single substitution only on non-ligated glyphs, first ligature in font order) |
| subsetter 0.2 | MIT/Apache | no | remaps glyph ids; the PDF uses Identity-H with CID = GID |
| hypher, hyphenation | MIT/Apache | no | no exceptions, different lowercase rule, own pattern snapshots compiled in |
| unicode-normalization / icu_normalizer | MIT/Apache / Unicode-3.0 | not needed here | patterns are NFC by contract (G7); NFC of document words is uo-docx-layout's need |
| **rustc-hash 2** | MIT/Apache | **yes** | the spike's lesson: classic FxHash piles `g1·65536 + g2` keys into the same buckets (61 → 33 ms with the v2 rotation); ~0 wasm bytes |
| uo-jsnum (own) | MIT | later (G8, revised after the skeptic) | the private `js` module moves there after uo-jsnum's gate 2 |

Estimated size of the own port in wasm: +60–120 KB raw (+20–40 KB brotli), without hyphenation
data and without `ooxml` (reuse report, extrapolated from the spike's 57 KB slice).

### Module layout and port order (TS file → Rust module)

`#![forbid(unsafe_code)]`. *(revised after the skeptic)* Dependencies: `rustc-hash` (pinned)
only. Features: `hyphen-data` (embeds the 7 pattern files, for native/.NET). Module 0 is the
private `js` module (R-JS), ported first.

| # | Rust module | From | Model | Notes |
|---|---|---|---|---|
| 0 | `js.rs` (private; revised after the skeptic) | JS semantics used by the TS (R-JS) | Sonnet | checked against V8 over every code point |
| 1 | `read.rs` (`View`, `OutOfBounds`, `Tag`) | `DataView` use everywhere; spike `read.rs` | Sonnet | big-endian reads over the **whole input**, offsets in `u64` with checked adds and `usize::try_from` (wasm32); one signed read path for the hmtx `-1` index |
| 2 | `limits.rs`, `error.rs` | — | Sonnet | `FontLimits`, `FontError`, `LimitKind` |
| 3 | `sfnt.rs` (`TableDir`, `is_font_file`) | sfnt.ts constructor loop; shared sfnt.ts `tableOf`, `FONT_SIGNATURES` | Sonnet | `last(tag)` (SfntFont, subset: `Map.set` overwrites) and `first(tag)` (shared `tableOf`) |
| 4 | `cmap.rs` | sfnt.ts `parseCmap`/`lookupGlyph`/`glyphId`; converter `parseCmap`; shared `glyphIdsFor` | Sonnet | three named readers (G6) |
| 5 | `face.rs`, `vdmx.rs`, `colr.rs` | sfnt.ts (constructor, metrics, `advanceWidth`, `vdmx`, `colrInfo`, `colorLayers`); fonts.ts `isSymbolFont` | Sonnet | |
| 6 | `gpos.rs` (`PairMap`, `Kerning`) | gpos.ts; sfnt.ts `parseLegacyKern`, kern lookup | Sonnet | work budget and pair cap (G5) |
| 7 | `gsub.rs` | gsub.ts; sfnt.ts `ligate` | Sonnet | |
| 8 | `outline.rs` | sfnt.ts `glyphOutline`, `glyphContours`, `contoursToPath` | Sonnet | |
| 9 | `program.rs` (`FontProgram`) | converter `TtfProgram`; C# `TtfProgram.ParseKern` | Sonnet | |
| 10 | `info.rs` | shared sfnt.ts `isEmbeddable`, `hasTrueTypeOutlines`, `faceInfo`, `nameString`, `postscriptName` | Sonnet | names kept as UTF-16 |
| 11 | `ttc.rs` | shared sfnt.ts `standaloneFace` | Sonnet | output cap |
| 12 | `subset.rs` | shared subset.ts | Sonnet | |
| 13 | `eot.rs` (`write` now, `read` later) | shared eot.ts | Sonnet | |
| 14 | `odttf.rs` | ooxml.ts `obfuscateOdttf`; docx-core `deobfuscateFont` | Sonnet | two key rules |
| 15 | `variant.rs`, `catalog.rs` | types.ts, catalog.ts | Sonnet | catalog tables generated once (G9) |
| 16 | `hyphen/{mod,liang,registry,cache}.rs` + `data/hyphen/*.txt` | liang.ts, hyphen/index.ts, patterns | Sonnet | G7 |
| 17 | `embed.rs` | embedding.ts | Sonnet | *(revised after the skeptic)* two phases, app-neutral policy (R-EMBED) |
| ~~18~~ | ~~`ooxml/`~~ | ooxml.ts | — | *(revised after the skeptic, S17)* deferred to uo-docx/uo-pptx (G2); the spec table below goes with it, Opus there |

### Data model and ownership

Memory scopes: **per face** (a `Face` lives as long as any document or provider holds it; the TS
equivalent lives with the `SfntFont` and its bytes), **per hyphenator** (one per language per
engine), **per document session** (hyphenation cache), **per operation** (subset, EOT, plan,
embedding plans: outputs owned by the caller, nothing kept).

- `Face(Arc<FaceData>)`: `Clone`, `Send + Sync`. `FaceData` owns `bytes: Arc<[u8]>` (shared with
  the host and with uo-pdf, never copied), `dir: TableDir` (`Vec<TableRecord { tag, offset: u32,
  length: u32 }>` in file order), `metrics: FontMetrics`, `num_glyphs: u16`,
  `number_of_h_metrics: u16`, `hmtx_offset: u32`, `cmap: Cmap`, and lazily parsed parts in
  `OnceLock`: `kerning: OnceLock<Kerning>`, `gsub: OnceLock<Option<GsubLigatures>>`,
  `vdmx: OnceLock<Option<VdmxTable>>`, `colr: OnceLock<Option<ColrInfo>>`; plus
  `outlines: Mutex<OutlineCache>`, and *(revised after the skeptic, S11f)* `limits: FontLimits`
  (a copy: the lazy parses need it). Lazy parsing does not change results (every parse is a pure
  function of the bytes, failures are caught at the same points as the TS); the TS parsed GPOS
  in the constructor, the Rust parses it on the first kerning query.
- No global cache. The TS `WeakMap<Uint8Array, GposKern>` (one parse per bytes object) becomes
  "one `Face` per bytes": `Face` is cheap to clone; the bindings dedupe per buffer (the TS shell
  keeps a `WeakMap<Uint8Array, FaceHandle>`). The TS advance cache is dropped (an hmtx read is
  O(1) and the cache made the spike's font `!Sync`). *(revised after the skeptic, S16)* On the
  native/.NET side the dedupe is a **capped `FaceCache`** owned by the Rust layout provider
  (keyed by the bytes' identity/digest, LRU by bytes held) — specified in the uo-docx-layout plan;
  uo-fonts only guarantees that a `Face` is cheap to clone and share.
- *(revised after the skeptic, S16)* **Compact cmap**: `Cmap { kind: CmapKind, order: Box<[u32]>,
  symbol: bool }` with `enum CmapKind { Format4 { end_p: u64, start_p: u64, delta_p: u64,
  range_p: u64 }, Format12 { groups_p: u64 } }`; `order` holds the segment/group **indices**
  stable-sorted by their start (4 B per range: Meiryo's 14,335 ranges take 57 KB instead of
  ≈ 460 KB). `start`, `end`, `delta`, `rangeOffset`, `startGlyph` are re-read from the bytes at
  query time; those reads cannot fail, because the parse performed every one of them in the TS
  order (R-READS). Only the format-4 glyphIdArray read can fail at query time (G4 → 0).
- `Kerning { pairs: PairMap, class0: FxHashMap<u16, Class0>, class_defs: Vec<ClassDef>,
  kern_in_latin: bool }`, `Class0 { adv: i16, cd2: u32 /* index into class_defs */ }`,
  `ClassDef { map: FxHashMap<u32, u16> }` (keys up to **131,069** = 65,535 + 65,534: format 1
  `start + i`; revised after the skeptic, S11i). *(S11h)* `class_defs` keeps only the cd2 maps
  referenced by a class-0 entry (the TS frees the others); the parse builds every cd2 and moves
  into `class_defs` only those a new class-0 entry points to.
  `PairMap` stores the TS key `k = g1·65536 + g2` (g2 may exceed 0xFFFF) canonically:
  `map: FxHashMap<u32, i16>` keyed `((k >> 16) << 16) | (k & 0xFFFF)` for `k >> 16 <= 0xFFFF`,
  and `overflow: FxHashSet<u16>` for the keys with `k >> 16 == 0x10000` (unreachable by lookups,
  but they occupy a slot of the TS `Map`, count in its size and so in the 2^24 limit and in the
  "GPOS gave no pairs → legacy kern" test). `len() = map.len() + overflow.len()`;
  `insert_if_absent(k: u64, v: i16) -> Result<(), MapFull>`. Collisions of the TS key are
  reproduced exactly (they are the same number).
- `GsubLigatures { features: Vec<(Tag, Vec<u16>)> /* first insertion order of the tag */,
  lookups: FxHashMap<u16, LigLookup>, singles: FxHashMap<u16, FxHashMap<u16, u16>> }`,
  `LigLookup = FxHashMap<u16, Vec<Ligature>>`, `Ligature { rest: Box<[u16]>, lig: u16 }` (font
  order).
- `OutlineCache`: `FxHashMap<u32, Option<Arc<[OutlineCmd]>>>`, only for `gid < num_glyphs`
  (larger ids are `None` without caching), cleared entirely when it holds
  `FontLimits::outline_cache_entries` (default 2,048) and a new glyph is added — the TS cache
  was unbounded (a CJK font can hold 65,535 outlines). `OutlineCmd` is an enum of f64 fields.
- `Hyphenator { id: String, left: u32, right: u32, trie: Trie, exceptions: FxHashMap<String,
  Box<[u32]>> }`, immutable, `Send + Sync`, shared by `Arc`. `Trie` is built with
  `Vec<FxHashMap<u32, u32>>` children and frozen into CSR arrays (`first_edge: Vec<u32>`,
  `edges: Vec<(u32 cp, u32 node)>` sorted by code point, `values: Vec<Option<Box<[u8]>>>`);
  lookup by binary search. *(revised after the skeptic, S12)* The cache is bound to its
  hyphenator: `HyphenSession { hyphenator: Arc<Hyphenator>, cache: FxHashMap<String, Box<[u32]>> }`
  is owned by the caller per document and language, and `points`/`hyphenate` with a cache exist
  only on the session (TS: the cache belongs to the instance, liang.ts:42; results do not depend
  on it). `HyphenationRegistry { loaded: Vec<Arc<Hyphenator>> }` per engine instance (TS: a
  module global `Map`); `register` **replaces** an existing id in place (S11c, `Map.set`).
- *(revised after the skeptic, S11g)* `ColrInfo { base: u64, n_base: u16, layers: u64,
  n_layers: u16, colors: u64, first_color: u16, n_colors: u16 }` — the TS sums of `u32` offsets
  do not wrap.
- Operation outputs (`subset_true_type`, `standalone_face`, `sfnt_to_eot`, `odttf`, plans) are
  owned `Vec<u8>`/`Cow<[u8]>` returned to the caller; `EmbeddingPlan` holds `Arc<[u8]>` of the
  face bytes (shared with the source) or the owned subset.

### Public API

```rust
// read.rs / sfnt.rs
pub struct Tag(pub [u8; 4]);                      // Tag::from_str("liga") -> Option<Tag> (4 bytes)
pub fn is_font_file(bytes: &[u8]) -> bool;

// limits.rs / error.rs
// (revised after the skeptic: S1 max_outline_work, S8 max_cmap_ranges, S11f DEFAULT by target)
#[derive(Clone, Copy)]
pub struct FontLimits { pub max_kern_pairs: u32, pub max_table_work: u64,
    pub max_outline_work: u32, pub max_outline_points: u32, pub outline_cache_entries: u32,
    pub max_cmap_ranges: u32, pub max_output_bytes: u64 }
impl FontLimits { pub const NATIVE: Self; pub const WASM32: Self;
    pub const DEFAULT: Self; /* = WASM32 when cfg(target_pointer_width = "32"), else NATIVE */ }
pub enum FontError { OutOfBounds, MissingTable(Tag), NoCmapSubtable, CmapFormat(u16),
    TableOutsideFile, Limit(LimitKind) }
pub enum LimitKind { KernPairs, TableWork, OutlineWork, OutlinePoints, CmapRanges, OutputBytes }

// face.rs
pub struct FontMetrics {
    pub units_per_em: u16, pub ascender: i16, pub descender: i16, pub line_gap: i16,
    pub us_win_ascent: u16, pub us_win_descent: u16,
    pub s_typo_ascender: i16, pub s_typo_descender: i16, pub s_typo_line_gap: i16,
    pub fs_selection: u16,
    pub code_page_range1: Option<u32>,          // OS/2 length >= 86 && version >= 1
    pub y_superscript_y_offset: Option<i16>,    // OS/2 length >= 26
    pub x_avg_char_width: Option<i16>,          // OS/2 length >= 4
    pub panose: Option<[u8; 10]>,               // OS/2 length >= 42
    pub underline_position: i16, pub underline_thickness: i16,   // 0 unless post length >= 12
}
#[derive(Clone)] pub struct Face(/* Arc<FaceData> */);
impl Face {
    pub fn parse(bytes: Arc<[u8]>, limits: &FontLimits) -> Result<Face, FontError>;
    pub fn bytes(&self) -> &Arc<[u8]>;
    pub fn metrics(&self) -> &FontMetrics;
    pub fn num_glyphs(&self) -> u16;
    pub fn glyph_id(&self, code_point: u32) -> u32;          // saturating (see traps), 0 = .notdef
    pub fn advance_width(&self, glyph_id: u32) -> u16;       // font units; G4 fallback 0
    /// The Office kerning emulation (revised after the skeptic, S19): union of the `kern`
    /// features of every script, first value wins across lookups, first-glyph XAdvance only,
    /// lookup flags ignored, the class-0 rule, legacy `kern` when GPOS gave no pair. Not
    /// OpenType shaping. 0 = none; g1 or g2 > 0xFFFF -> 0.
    pub fn kern_units(&self, g1: u32, g2: u32) -> i32;
    pub fn kern_in_latin(&self) -> bool;
    pub fn ligate(&self, gids: &[u32], features: &[Tag]) -> Option<Vec<LigGroup>>;  // LigGroup { gid: u32, len: u32 }
    pub fn is_symbol_font(&self) -> bool;
    pub fn vdmx(&self, ppem: u16) -> Option<VdmxEntry>;      // VdmxEntry { y_max: i16, y_min: i16 }
    pub fn has_color_glyphs(&self) -> bool;
    pub fn color_layers(&self, glyph_id: u32) -> Option<Vec<ColorLayer>>;  // ColorLayer { glyph_id: u16, color: Option<[u8; 3]> /* RGB */, alpha: f64 }
    pub fn glyph_outline(&self, glyph_id: u32) -> Option<Arc<[OutlineCmd]>>;
    pub fn table(&self, tag: Tag) -> Option<TableRecord>;    // last record with the tag
}
pub enum OutlineCmd { M { x: f64, y: f64 }, L { x: f64, y: f64 }, Q { cx: f64, cy: f64, x: f64, y: f64 }, Z }

// program.rs (converter TtfProgram)
pub struct FontProgram { /* Arc<[u8]>, cmap (program scoring), hmtx, metrics below */ }
impl FontProgram {
    pub fn parse(bytes: Arc<[u8]>, limits: &FontLimits) -> Result<FontProgram, FontError>;
    pub fn units_per_em(&self) -> u16; pub fn ascender(&self) -> i16; pub fn descender(&self) -> i16;
    pub fn font_bbox(&self) -> [i16; 4]; pub fn italic_angle(&self) -> f64;
    pub fn glyph_id(&self, code_point: u32) -> u32; pub fn advance_width(&self, glyph_id: u32) -> u16;
    /// C#-only reader (TtfProgram.cs:170–201), lazy in a OnceLock, parity from a C# capture (G16).
    pub fn legacy_kern(&self, left: u32, right: u32) -> i32;
}

// info.rs / ttc.rs / subset.rs / eot.rs / odttf.rs
pub fn is_embeddable(bytes: &[u8]) -> bool;
pub fn has_true_type_outlines(bytes: &[u8]) -> bool;
// revised after the skeptic (S13, G4): Result, so a malformed cmap fails the plan like the TS
pub fn glyph_ids_for(bytes: &[u8], code_points: impl IntoIterator<Item = u32>, limits: &FontLimits)
    -> Result<Vec<u32>, FontError>; // sorted, distinct, no 0
pub struct FaceInfo { pub panose: String /* upper hex, 20 chars or fewer when clamped at EOF (S10) */, pub weight: u16, pub italic: bool,
    pub fs_type: u16, pub unicode_range: [u32; 4], pub code_page_range: [u32; 2],
    pub check_sum_adjustment: u32, pub names: FaceNames }
pub struct FaceNames { pub family: Vec<u16>, pub style: Vec<u16>, pub version: Vec<u16>, pub full: Vec<u16> } // UTF-16 code units
pub fn face_info(bytes: &[u8]) -> Result<FaceInfo, FontError>;
// `Some("")` behaves as `None` (the TS `if (wantPostscript)`, S11a). The owned output can be
// turned into `Arc<[u8]>` by the caller without a second copy (`Arc::from(vec)`) (S16).
pub fn standalone_face<'a>(bytes: &'a [u8], want_postscript: Option<&str>, limits: &FontLimits) -> Result<Cow<'a, [u8]>, FontError>;
// revised after the skeptic (S3): limits parameter; over max_output_bytes -> Cow::Borrowed(input)
pub fn subset_true_type<'a>(bytes: &'a [u8], used_gids: impl IntoIterator<Item = u32>, keep_layout: bool,
    limits: &FontLimits) -> Cow<'a, [u8]>;
pub fn sfnt_to_eot(font: &[u8], subsetted: bool) -> Result<Vec<u8>, FontError>;
// revised after the skeptic (S4): keys are processed as UTF-16 code units (R-ODTTF)
pub mod odttf { pub fn obfuscate(data: &[u8], font_key: &str) -> Vec<u8>;
                pub fn deobfuscate<'a>(data: &'a [u8], font_key: &str) -> Cow<'a, [u8]>;
                pub fn obfuscate_utf16(data: &[u8], font_key: &[u16]) -> Vec<u8>;           // bindings pass JS strings as code units
                pub fn deobfuscate_utf16<'a>(data: &'a [u8], font_key: &[u16]) -> Cow<'a, [u8]>; }

// variant.rs / catalog.rs
pub enum Variant { Regular, Bold, Italic, BoldItalic }    // VARIANTS in this order
pub fn variant_of(bold: bool, italic: bool) -> Variant;
pub fn variant_of_style(style: Option<&str>) -> Option<Variant>;
pub const DEFAULT_FAMILY: &str;                              // "Liberation Sans"
pub fn bundled(family: &str) -> Option<BundledFamily>;       // own keys only (G9)
pub fn bundled_file_name(file: &str, v: Variant) -> String;
pub fn metric_substitute(family: &str) -> Option<&'static str>;
pub const METRIC_SUBSTITUTES: &[(&str, &str)]; pub const PROBE_FAMILIES: &[&str]; // TS order, duplicates kept

// embed.rs — revised after the skeptic (S13, S19): two phases, app-neutral policy (R-EMBED)
pub struct EmbeddingPolicy<'a> {
    pub embed: bool, pub subset: bool,
    pub skip_common_fonts: bool,          // the TS `app === 'slides' || settings.skipCommonFonts`, resolved by the caller
    pub common_fonts: &'a [&'a str],      // compared lowercased
    pub all_variants: bool,               // true: the four VARIANTS (PowerPoint); false: the used ones (Word)
}
// the measured Office tables stay as consts (the per-app policy is the caller's)
pub const WORD_COMMON_FONTS: [&str; 3];          // Arial, Times New Roman, Courier New
pub const POWERPOINT_COMMON_FONTS: [&str; 5];    // + Calibri, Aptos
pub struct FontEmbeddingSettings { pub embed: bool, pub subset: bool, pub skip_common_fonts: bool }
pub const WORD_DEFAULT_EMBEDDING: FontEmbeddingSettings;        // false, false, true
pub const POWERPOINT_DEFAULT_EMBEDDING: FontEmbeddingSettings;  // false, true, true
pub struct FontUsage<'a> { pub family: &'a str, pub variants: &'a [Variant], pub text: &'a str }
pub struct FaceRequest { pub family: String /* first spelling */, pub variant: Variant }
/// Phase 1: exactly the (family, variant) pairs for which the TS would call
/// `documentFace` / `loadFace`, in the TS order. Empty when `!policy.embed`.
pub fn embedding_requests(usage: &[FontUsage<'_>], policy: &EmbeddingPolicy<'_>,
    already_embedded: &FxHashSet<String> /* lowercased */) -> Vec<FaceRequest>;
/// Phase 2: `faces` answers each request (document face ?? own file ?? loaded; `None` = no file).
pub fn plan_font_embedding(usage: &[FontUsage<'_>], policy: &EmbeddingPolicy<'_>,
    already_embedded: &FxHashSet<String>, faces: &dyn Fn(&FaceRequest) -> Option<Arc<[u8]>>,
    limits: &FontLimits) -> Result<EmbeddingPlan, FontError>;
pub struct FaceToEmbed { pub family: String, pub variant: Variant, pub bytes: Arc<[u8]>, pub subsetted: bool, pub info: FaceInfo }
pub enum SkipReason { Common, License, Unavailable, AlreadyEmbedded }
pub struct EmbeddingPlan { pub faces: Vec<FaceToEmbed>, pub skipped: Vec<(String, SkipReason)> }

// ooxml: deferred to uo-docx / uo-pptx (G2, revised after the skeptic, S17)

// hyphen
pub struct HyphenationPatterns<'a> { pub id: &'a str, pub left: u32, pub right: u32, pub patterns: &'a str, pub exceptions: &'a str }
pub const HYPHENATION_PATTERN_IDS: [&str; 7];              // pt, en-us, es, fr, de-1996, it, nl
pub fn hyphenation_pattern_id(lang: Option<&str>) -> Option<&'static str>;
pub fn pattern_ids_to_load<'a>(langs: impl IntoIterator<Item = Option<&'a str>>, loaded: &HyphenationRegistry) -> Vec<&'static str>;
pub struct Hyphenator; impl Hyphenator {
    pub fn new(p: &HyphenationPatterns<'_>) -> Hyphenator;
    pub fn id(&self) -> &str; pub fn left(&self) -> u32; pub fn right(&self) -> u32;
    pub fn points(&self, word: &str) -> Vec<u32>;                        // uncached
    pub fn hyphenate(&self, word: &str, left: u32, right: u32) -> Vec<u32>;
}
// revised after the skeptic (S12): the cache is bound to its hyphenator
pub struct HyphenSession { /* Arc<Hyphenator>, cache */ } impl HyphenSession {
    pub fn new(h: Arc<Hyphenator>) -> HyphenSession; pub fn hyphenator(&self) -> &Arc<Hyphenator>;
    pub fn points(&mut self, word: &str) -> Vec<u32>;
    pub fn hyphenate(&mut self, word: &str, left: u32, right: u32) -> Vec<u32>;
}
pub struct HyphenationRegistry;   // register(p) -> Arc<Hyphenator> (replaces the same id, S11c), for_lang(lang)
#[cfg(feature = "hyphen-data")] pub fn embedded_patterns(id: &str) -> Option<HyphenationPatterns<'static>>;

pub fn clear_caches();   // no process-wide cache: a no-op, kept for the memory harness
```

What crosses to JS/.NET later (bindings, not this module): `Face`/`FontProgram`/`Hyphenator`/
`HyphenSession` handles (created from bytes/strings, freed explicitly), batch queries
(`glyph_ids`, `advance_widths`, `kern_units` over arrays — one crossing per run of text, not per
glyph), outlines as flat `f64` arrays, byte-in/byte-out functions (subset, EOT, odttf, .ttc,
face info), the catalog tables. 0.10 API changes for `CHANGELOG.md`: `SfntFont`,
`LayoutFont`/`ILayoutFont` and `TtfProgram` become `Face`/`FontProgram`; twips scaling and
advance tables move to the layout API; glyph ids are `u32`; name strings are UTF-16 code units;
hyphenation patterns must be NFC; *(revised after the skeptic)* the embedding plan is two-phase
with an app-neutral policy (no app enum); the hyphenation cache lives in a `HyphenSession`;
`subset_true_type`/`glyph_ids_for` take `FontLimits`; the docx/pptx writers (and their injected
GUID source) move to the format crates.

### Reading model and the TS catch points (the rule every module follows)

`View` reads through **one view over the whole input** (`DataView(bytes.buffer, byteOffset,
byteLength)`): a table shorter than its fields reads the following bytes; only a read past the
end fails (`OutOfBounds`, the `RangeError`). Where the TS catches decides the result:

| Where | TS on failure | Rust |
|---|---|---|
| `new SfntFont`: directory, `req` (head, hhea, maxp, OS/2, hmtx, cmap — in this order), eager metric reads, `parseCmap` | throws: the font is rejected | `Face::parse` → `Err` with the same class, **same order of checks** |
| GPOS (`parseGposKern` + `gposKernInLatin`, one `try`) | empty pairs, class 0 discarded, `kernInLatin` keeps `true` | the same, atomic |
| legacy `kern` (only when the GPOS gave 0 pairs) | **partial**: pairs inserted before the failure stay | the same (`legacy_pairs` stops at the error and keeps what it read) |
| `ligate` → `parseGsubLigatures` | `null` for the life of the font | `None`, cached |
| `vdmx`, `colrInfo`, `glyphOutline` | `null`/`undefined` | `None` |
| `glyphId` (format-4 glyphIdArray read), `advanceWidth`, `colorLayers` | **uncaught**: the layout fails | G4: total functions (0 / `None`), accepted by name |
| shared `isEmbeddable` / `hasTrueTypeOutlines` | `true` / `false` | the same |
| `faceInfo`, `standaloneFace`, `glyphIdsFor` | uncaught (the embedding plan rejects) | `Result` for all three; *(revised after the skeptic, S13)* `glyph_ids_for` is **not** partial: its `Err` fails `plan_font_embedding`, like the TS |
| `subsetTrueType` | the original bytes | `Cow::Borrowed(bytes)` |

Hostile-input limits that the TS does not have (G5, G11; revised after the skeptic): the work
budget `max_table_work` counted at the points listed in R-BUDGET (every unbounded loop, not only
GPOS/GSUB), `max_kern_pairs` (the V8 `Map` limit), `max_outline_work` (component visits per
`glyph_outline` call, S1) and `max_outline_points`, `max_cmap_ranges` (the JS array limit, S8),
`max_output_bytes` (`standalone_face`, `subset_true_type`, S3). Over a limit, each function gives
**its own TS catch outcome** (table below in R-BUDGET).

### JS semantics, per file (what the Sonnet port must reproduce; each is a parity facet)

**sfnt.ts (face, cmap, vdmx, colr, outline)**
- Table directory: `Map.set` → the **last** record of a tag wins; shared `tableOf` → the
  **first**. Tags are 4 Latin-1 chars (`String.fromCharCode` of bytes): compare as bytes.
- cmap choice: score (3,10)=4, (3,1)=3, (0,*)=2, (3,0)=1, other 0; strictly greater wins (the
  first of equal scores). The chosen subtable may be format 14 or 6 → the font is rejected
  (`CmapFormat`). `symbol = bestScore == 1`.
- Format 4: `segCount = u16/2` is a **float** for an odd value: the loop runs `ceil(segX2/2)`
  times, but `startP = endP + segCount*2 + 2` uses the exact `segX2` (integer). Ranges are
  pushed in table order and **stable-sorted by start** (`Array.prototype.sort`, stable; Rust
  `sort_by_key` is stable). Lookup is the exact binary search (`mid = (lo + hi) >> 1`, `lo`/`hi`
  as `i64`): with overlapping ranges it returns whatever range that search hits. Delta mapper
  `(c + delta) & 0xFFFF` in `i32`; array mapper reads `u16` at `id_p + range_offset + (c −
  start)·2` at query time, `g == 0 ? 0 : (g + delta) & 0xFFFF`.
- Format 12: `startGlyph + (c − start)` is **not** wrapped (a JS number up to 2^33): return it
  saturated to `u32::MAX`. Every consumer treats any id above 0xFFFF or ≥ `numGlyphs` the same
  way (kerning 0, PDF `.notdef`, outline `None`, ligature no match, subset filter, hmtx last
  metric), so saturation keeps every observable result.
- `glyphId`: when the lookup gives 0 and `symbol && cp < 0x100`, look up `cp + 0xF000`.
- `advanceWidth`: `i = min(gid, numberOfHMetrics − 1)` in **signed** arithmetic: with
  `numberOfHMetrics == 0` the index is −1 and the TS reads the 2 bytes at `hmtxOffset − 4` (the
  previous table) without error. Reproduce exactly; only a read outside the input falls back (G4).
- Metrics: optional fields exactly by `os2.length` and version; OS/2 fields read whatever bytes
  follow a short OS/2. `codePageRange1` is `u32` (`getUint32`; consumers use `>>> 17`, `>>> 31`).
- `isSymbolFont`: `codePageRange1 >>> 31 == 1` (absent → 0) **or** `glyphId(0xF020) != 0`.
- VDMX: the first ratio with `x == 0 || (y0 <= x && x <= y1)` (the charset byte ignored); records
  inserted with `Map.set` → **last** record of a ppem wins; the whole table is `None` on any
  failure; only integer ppem keys match.
- COLR: `colrInfo` requires COLR and CPAL lengths ≥ 14 and non-zero `nBase`, base and layer
  offsets; `colorLayers` binary-searches the base records, iterates `k < n && first + k <
  nLayers`, palette index `0xFFFF` or `>= nColors` → no color, alpha 1; else BGRA → RGB and
  `alpha = a / 255` (f64 division). An empty layer list → `None`.
- *(revised after the skeptic, S1)* Outline work: every entry into `glyph_contours` (the root
  and each component, at any depth) counts one visit; when the count exceeds
  `max_outline_work` (default 4,096) the whole `glyph_outline` returns `None` (cached). The TS
  hangs on a self-referencing composite (64^9 visits, zero points), so this is G11, accepted by
  name; points beyond `max_outline_points` → `None` too.
- Outline: depth > 8 → `None` for that component; composite loop at most 64 components; args
  as words or bytes; `!(flags & 2)` zeroes the offsets; F2Dot14 = `i16 / 16384`; transform
  `x' = a·x + c·y + dx`, `y' = b·x + d·y + dy` in this order, f64, **no fused multiply-add**.
  Simple glyph: flags with repeat, deltas; contour ranges come from the end-points array as
  written (not sorted): a decreasing end gives an empty contour and restarts the next one
  earlier. An end ≥ `nPts` makes the TS emit `undefined` coordinates: Rust returns `None` for
  that glyph (accepted by name, G4). `contoursToPath`: contours with fewer than 2 points are
  skipped; start at the first on-curve point, else the midpoint `(p0 + p1)/2` with start index
  0; implicit on-curve midpoints `(ctrl + q)/2`; closing `Q` back to the start; `Z`.
- Empty glyph (`end <= start` in loca) → `Some([])` (cached), not `None`.

**gpos.ts / legacy kern**
- Feature indices with tag `kern` in feature-list order; lookup indices in a `Set` →
  **insertion order** (feature order, then the order listed in each feature, first occurrence
  kept); lookups `>= lookupCount` skipped; type 9 extension reads its type and `u32` offset.
- Coverage format 1 list / format 2 ranges expanded `start..=end` (an inverted range adds
  nothing); **any other format → empty** (GSUB differs, below).
- PairPos format 1: only `i < min(pairSetCount, coverage.len())`; each record `2 + size1 +
  size2` bytes; XAdvance read only if `vf1 & 4`, after `XPlacement`/`YPlacement` when present;
  adv 0 skipped; `insert_if_absent` (first wins across all subtables and lookups).
- PairPos format 2: class defs with `Map.set` (format 1 keys `start + i`, up to 131,069; format 2
  ranges; **last value wins, first insertion position kept**); the `c1n × c2n` class matrix is
  walked eagerly cell by cell (each cell counts one unit of work); a cell **reads** only when
  `vf1 & 4` (after skipping XPlacement/YPlacement), so with `vf1 & 4` every read cell must be in
  bounds or the whole GPOS fails, and without it no cell is read (revised after the skeptic:
  pseudo-code in R-GPOS); `class1` = coverage glyphs in
  **first-occurrence order** mapped to `cd1.get(g) ?? 0`; class 0: for each `(g1, c1)` with
  `classAdv[c1][0] != 0` and g1 not yet in class 0 → `(adv, cd2 of this subtable)`; pairs: for
  each `(g1, c1)` in class1 order × each `(g2, c2)` in cd2 insertion order, `classAdv[c1]?.[c2]
  ?? 0` (out-of-range class → 0), non-zero → `insert_if_absent`. A g2 listed in cd2 **with class
  0** still produces pairs.
- Lookup (`kern_units`): either id > 0xFFFF → 0; `pairs.get(k)`; if absent, class 0 of g1 applies
  when `g2` is **not a key** of its cd2; the value 0 or absent → 0 (the TS `!units`).
- `gposKernInLatin`: script `latn`, else `DFLT` (the last record of each tag wins, as the loop
  overwrites); no script or `DefaultLangSys` offset 0 → false; required feature index `0xFFFF`
  is none.
- Legacy `kern`: version 0 only; subtables with `format == 0`, horizontal, not cross-stream;
  `p += length` (u16, may be 0: the same subtable again); `insert_if_absent`, partial on failure.
- The `Map` size limit (2^24 measured) applies to `pairs` (G5): reaching it is the TS
  `RangeError` → GPOS unreadable (atomic) / legacy partial.

**gsub.ts**
- Script `latn` else `DFLT` (last record wins); features of `DefaultLangSys`: required index if
  not `0xFFFF`, then the listed ones; `features` keyed by tag: a repeated tag **appends** its
  lookup list (duplicates kept); `wanted` = all lists flattened, distinct, in that order.
- Coverage: format 1 list; **any other format is read as format 2** (GPOS returns empty).
- SingleSubst format 1 `(g + d) & 0xFFFF` (`d` i16) over **every** coverage glyph; format 2 for
  `k < min(n, cov.len())`; `Map.set` → **last** subtable wins for a glyph. Ligature format 1 only:
  for `k < min(n, cov.len())`, ligature sets appended to the glyph's list in subtable order
  (first match wins later); component count `cc` → `cc − 1` rest glyphs (0 or 1 → empty rest:
  the ligature replaces one glyph).
- A lookup with both single and ligature subtables lands in both maps; `applyLigatures` uses the
  singles and skips the ligatures (`continue`).
- `ligate` returns `None` when no lookup or single exists, or when none of the requested features
  maps to a lookup in either map; then `applyLigatures`: lookup indices of the requested
  features, distinct, **sorted numerically**; singles apply only to `len == 1` groups; a
  ligature matches when every rest glyph equals the following groups' `gid`; the new `len` is the
  sum of the consumed groups' `len`.

**liang.ts / hyphen/index.ts**
- Patterns and exceptions split on JS whitespace (`/\s+/`, Unicode set, `\u{FEFF}` included —
  not `split_whitespace`); empty tokens skipped; inputs are NFC by contract (the TS normalized at
  runtime; the shipped data is checked NFC in the capture).
- Pattern parse: digits `'0'..='9'` set the value **after** the last letter (overwrite); values
  array `letters + 1`; a repeated pattern **replaces** the node's values.
- Exceptions: positions counted in code points; key = text without `-`, full-string lowercase
  (Rust `str::to_lowercase`, Final_Sigma included); a repeated key: **last wins**.
- `lowerCp(ch)`: lowercase of one code point; keep it when it is exactly one code point, or when
  its UTF-16 length equals the original's (then its first code point); otherwise the original
  (U+0130 stays U+0130). The rule goes in the private `js` module (revised after the skeptic,
  S15) and is checked over all code points against V8 (Unicode version trap: V8/ICU vs Rust std).
- `points`: `w = [., low…, .]`, `vals = [0; w.len() + 1]` (u8), max-merge along the trie from
  every start; `k` in `1..n` with `vals[k + 1]` odd; exception hit → its positions filtered to
  `0 < k < n`. `hyphenate`: `k >= left && n − k >= right`, `n` in code points.
- Cache: clear everything when it holds **more than 50,000** entries, before inserting. Words of
  any length are accepted (the TS `String.fromCodePoint(...low)` throws past V8's argument
  limit; accepted by name).
- `hyphenationPatternId`: Unicode lowercase, split on the first `-` or `_`, base language table;
  `loadHyphenation` ids: a lang that **is** an id is taken as is — own keys only (the TS
  `LOADERS[l]` matched `constructor`/`toString` and then threw; G9-style fix, accepted by name).

**shared sfnt.ts (info, ttc, glyphIdsFor)**
- `glyphIdsFor`: its own cmap: score (3,10)=3, (3,1)=2, (0,*)=1, else 0 (**no (3,0)**), formats
  4 and 12 only (others → no glyphs); format 4 lookup is a **linear scan in table order**: the
  first segment with `cp <= end` decides (`cp < start` → 0), `cp > 0xFFFF` → 0; format 12 linear,
  first group containing `cp`; ids 0 dropped; result is a set.
- `faceInfo`: panose upper-case hex of `bytes.subarray(os2.off + 32, os2.off + 42)` — **clamped
  at the end of the input** (revised after the skeptic, S10: a table cut by EOF gives fewer than
  20 chars, possibly `""`) — or `"0"×20` when OS/2 < 42 bytes; `weight || 400`;
  `codePageRange` only for OS/2 version ≥ 1; `u32` fields 0 when the table is too short;
  names: platform 3 only, the en-US (0x409) record wins, else the **first non-empty** one
  (`any ||= s`); UTF-16BE code units, an odd length drops the last byte; lone surrogates kept
  (hence `Vec<u16>`).
- `postscriptName` (for `.ttc`): name ID 6, platforms 3 and 0 as UTF-16, others as Latin-1
  bytes, the first non-empty; compared with `want_postscript` as UTF-16 code units.
- `standaloneFace`: not `ttcf` → the input; with a non-empty `want_postscript` (`Some("")` is
  `None`, S11a) the `numFonts` faces are scanned for the PostScript name (else face 0; each
  face × name record counts one unit of work, R-BUDGET); a table with `off + len > bytes.len()` → `TableOutsideFile`; output size =
  header + Σ `(len + 3) & ~3` — in JS `&` is **int32**: lengths ≥ 2^31 − 3 go negative (the
  `Uint8Array` throws); Rust checks against `max_output_bytes` first (a directory pointing
  every table at the whole file multiplies the input up to 65,535×).

**subset.ts**
- Directory into a `Map` (last record wins); requires head, maxp, loca, glyf (else the input).
- `glyphOff[i]` for `i in 0..=numGlyphs` (loca short ×2 / long); keep = {0} ∪ used ids `< numGlyphs`
  ∪ composite components (worklist, LIFO): a component id ≥ `numGlyphs` is still added.
  *(revised after the skeptic, S2)* Offsets of glyphs past the loca array are **NaN** in the TS;
  the exact state machine (`Off::Nan`: reads at offset 0, `+ n` stays `Nan`, every comparison
  false) and its exit are in R-SUBSET. The plan's earlier "the component loop ends at the first
  read past the input" was false: in the `Nan` state with flag 0x20 the TS loops forever on
  offset 0; Rust returns the input (G11).
- New glyf: kept glyphs with `len > 0`, padded to even, copied with `subarray` (**clamped**, no
  error past the end; the missing bytes stay 0); loca long; head copied, `indexToLocFormat = 1`,
  `checkSumAdjustment = 0` (a head shorter than 52 bytes throws → the input); other tables
  copied with `slice` (clamped). *(revised after the skeptic, S3)* The output size is computed
  **before any allocation** (R-SUBSET); over `max_output_bytes`, or an int32-negative padded
  length, → the input.
- Output: tables sorted by tag bytes; `searchRange = 2^floor(log2 n)·16` (integer `ilog2`, no
  `pow`); checksums `u32` wrapping over the padded length; `checkSumAdjustment = 0xB1B0AFBA −
  fileSum` wrapping. Deterministic bytes (no time stamps): compared byte for byte.

**eot.ts**: little-endian fixed header (82 bytes), panose bytes from the hex, charset 1, italic,
weight, fsType, magic `0x504C`, ranges, checksum; names as **UTF-16LE code units**
(`charCodeAt`), padding words as written; `EOTSize`/`FontDataSize` at the end.

**odttf** (two rules, both kept): key hex = `fontKey` without `{`, `}`, `-`; byte `i` =
**JS `parseInt(hex.slice(2i, 2i + 2), 16)` stored into a `Uint8Array`** (`js::parse_int_hex`
then `js::to_uint8`: leading JS whitespace skipped, sign allowed, `0x` prefix allowed, digits
until the first invalid one, `NaN` → 0, `-1` → 255, `"1g"` → 1); key reversed; XOR of the first
`min(32, len)` bytes with `key[i % 16]`. `deobfuscate` (docx-core) returns the input unchanged
when the hex has fewer than 32 chars; `obfuscate` (fonts ooxml) does not check.
*(revised after the skeptic, S4)* All of this is in **UTF-16 code units** of the attribute
(R-ODTTF): the length check, the slices and `parseInt`.

**embedding.ts**: families grouped by lowercase key (insertion order, first spelling kept), the
union of variants and of the text's code points; `already_embedded` → skipped; common fonts
(`skipCommon = app == Slides || skip_common_fonts`) → skipped; wanted variants: all four in
`VARIANTS` order for Slides, else the used ones in `VARIANTS` order; `found`/`blocked` counters;
subset only with `settings.subset && has_true_type_outlines`, with `keep_layout`;
`info = face_info(original bytes)`. Rust text has no lone surrogates (TS `codePointAt` of a lone
surrogate could map through a font's cmap; accepted by name). *(revised after the skeptic)*
`subsetted` is `subset && has_true_type_outlines` **even when** `subset_true_type` fell back to
the input (S11b); the two-phase, app-neutral shape is in R-EMBED.

**ooxml.ts** — *(revised after the skeptic, S17)* **not ported in this module** (G2: deferred to
the uo-docx/uo-pptx plans; Opus there). The table stays here as the spec those plans start from
(every regex a hand matcher, linear time, checked by a differential fuzz against the TS regex
over generated XML text):

| TS | Exact semantics to reproduce |
|---|---|
| `relsPathOf`, `readRels` (`/<Relationship\b[^>]*\/?>/g`) | `uo_opc::rels` parser (accepted differences on malformed `.rels`: single quotes, entities, attributes split by `>` in values) |
| `addRel` | `first_free_rel_id(rels, "rId", distinct_ids + 1)` (the empty id counts as one distinct id), `add_rel(.., Insert::BeforeLastCloseTrimmed)`; target escaped `& < > "` only, `Id`/`Type` raw — check `Relationship::to_xml` matches |
| `removeRelsOfType` | `remove_rels` by type; each non-external target removed with the fonts rule (`/x` → `x`; split on `/`; `..` pops; `.` skipped; empty segments kept) during parity, `Package::resolve_target` in production (D6), differences accepted by name |
| `ensureDefaultContentType` (`<Default\b[^>]*Extension="ext"`, flag `i`) | for each ASCII-case-insensitive `<default` followed by a non-word char or the end, the text up to the next `>` (or the end) contains `extension="ext"` case-insensitively → present; else `add_default(.., Insert::AfterRootOpen)` |
| `ensureOverride` | `xml.contains("PartName=\"/{part}\"")` (case-sensitive) else `add_override(.., BeforeLastCloseTrimmed)` |
| `uniquePartName` | names lowercased (Unicode), `{dir}font{n}.{ext}` from n = 1 |
| `findElement` (`<qname(?=[\s/>])`) / `elementEnd` | first `<qname` followed by JS whitespace, `/` or `>`; end = after the first `>` if the char before it is `/`, else after the first `</qname>` after that `>`; none → not found |
| `readDocxEmbedding` `on(name)` | some `<w:name` followed by `[\s/>]` such that the run from just after the name up to the next `>` (or the end) does **not** contain `w:val="false"`, `w:val="0"` or `w:val="off"` (case-sensitive) |
| `/<w:font\b[^>]*w:name="([^"]*)"[^>]*>([\s\S]*?)<\/w:font>/g` | scan from `pos`: `<w:font` + non-word char; inside the tag (up to the first `>`) the **last** `w:name="` whose value is followed by `"`; content = up to the next `</w:font>`; a self-closing `<w:font …/>` therefore swallows the next font's content (TS behaviour); continue after the match |
| `/<w:embed(?:Regular\|Bold\|Italic\|BoldItalic)\b/` | one of the four names followed by a non-word char or the end |
| `table.replace(/<w:embed(...)\b[^>]*\/>/g, '')` | remove every such start tag whose first `>` is immediately preceded by `/` (from `<` to that `>`); a tag closed by a plain `>` is kept |
| `/<w:fonts\b[^>]*xmlns:r=/`, `/(<w:fonts\b[^>]*)\/>/` | within the first `<w:fonts` tag (up to `>`) |
| family element `new RegExp('<w:font\\b[^>]*w:name="F"[^>]*?(\\/>\|>[\\s\\S]*?<\\/w:font>)', 'i')` | case-insensitive match of the escaped family (`escAttr` then regex-escaped); the matched element is replaced by its first occurrence in the text; then `el.replace(/<\/w:font>$/, …)` is **case-sensitive**: an element ending in `</W:FONT>` gets no embeds (TS behaviour) |
| `/<\/w:fonts>\s*$/`, `/<\/Types>\s*$/`, `/<\/Relationships>\s*$/` | `Insert::BeforeLastCloseTrimmed` (JS whitespace) |
| `<w:settings\b[^>]*>` (self-closing expanded) and `SETTINGS_BEFORE` | insert after the largest end of those elements, else after the root start tag |
| pptx `<p:presentation\b[^>]*>` attribute edits; `\b{n}="([^"]*)"`, `/^(1\|true\|on)$/i`; `/<p:embeddedFont>\s*<p:font\b[^>]*typeface="([^"]*)"/g`; `PRES_AFTER` | as written: remove ` embedTrueTypeFonts="…"`/` saveSubsetFonts="…"` (preceded by one whitespace char), append before `/?>` with trailing whitespace dropped; flags by first attribute match; the typeface is the **last** `typeface="` in the tag |
| `randomGuid` | `guid()` bytes, version and variant bits set as the TS, upper-case `{8-4-4-4-12}` |
| `groupByFamily` | insertion order by exact family, faces stable-sorted by `VARIANTS` order |

**catalog.ts / types.ts**: object key order is insertion order (no integer-like keys); lookups
with own keys only (the TS indexed plain objects: `METRIC_SUBSTITUTES['constructor']` is a
function — the shell's resolver then throws; G9 fix, accepted by name); `bundledFileName` lowercases
the variant (`bolditalic`); `variantOfStyle`: Unicode lowercase, JS whitespace runs → one space,
JS trim; `''` → regular.

**Unicode and UTF-16, everywhere**: TS strings are UTF-16; Rust inputs are UTF-8. Where code
units are observable (names in EOT, `.ttc` PostScript names) they are kept as `Vec<u16>`;
text inputs (hyphenation words, usage text) cannot carry lone surrogates in Rust — accepted.

### uo-jsnum (answer to the uo-drawingml input)

Every `Math.*`, rounding and typed-array store in the code ported here:

| Use | Where | Rust |
|---|---|---|
| `Math.round` | `deviceEmPt` in `advanceTwips`/`kernGlyphsTwips` | **moves to uo-docx-layout** with the measuring model; `uo_jsnum::round` there |
| `Math.min` (integers) | `advanceWidth`, `Math.min(32, len)`, `Math.min(pairSetCount, cov.length)` | integer `min` on `i64`/`usize` (no NaN, no −0 possible) |
| `Math.floor(Math.log2(n))`, `2 ** k` | subset `buildSfnt` | `u32::ilog2`, `1 << k` (exact; no `pow`) |
| `/ 255`, `/ 16384`, `/ 65536` | COLR alpha, F2Dot14, `italicAngle` | plain f64 division (correctly rounded on every target) |
| `a·x + c·y + dx`, `(x + y) / 2` | outlines | plain f64 `*`/`+` in source order; `mul_add` banned |
| `>>> 0`, `& 0xFFFF`, `>>> 17`, `>>> 31`, `(n + 3) & ~3` | checksums, cmap, code pages, padding | `wrapping_add` on `u32`; `i32` masks; the int32 `& ~3` behaviour noted above |
| `parseInt(s, 16)` → `Uint8Array` | odttf keys | `js::parse_int_hex` over UTF-16 code units + `js::to_uint8` (R-ODTTF) |
| `parseInt(panose.slice(2i, 2i+2), 16)` → `Uint8Array` | EOT panose | *(revised after the skeptic, S10)* the hex is always valid, but clamped: byte `i` = the pair's value when `2i + 2 <= len`, else 0 (`parseInt('')` = NaN → 0); no general `parseInt` |
| `toString(16).padStart(2,'0').toUpperCase()` | panose hex, COLR color, GUID | `{:02X}` (bytes only) |
| `Uint8Array` values (hyphenation) | digits 0–9, max-merge | `u8`, no overflow possible (in-bounds proven by the indices) |
| `toLowerCase`, `/\s/`, `trim` | hyphenation, variant style, catalog/embedding keys | `js` module (R-JS) |

*(revised after the skeptic, S15)* uo-fonts does **not** depend on uo-jsnum: no transcendentals,
no `pow`, no `round` remain, and the few helpers it needs (R-JS) are a private `js` module that
moves to uo-jsnum after uo-jsnum's gate 2, when uo-jsnum offers code-unit `parse_int` and the
lowercase rules (its plan today has neither). The `disallowed-methods` lint does not exist yet in
`core/clippy.toml`; when it lands, it applies to uo-fonts (nothing to change: no banned method is
used).

### Revisions after the skeptic (the rules S1–S4, S7–S13, S18 written in)

**R-JS — the private `js` module (S15, S4)** (`src/js.rs`, `pub(crate)`):
- `is_js_space(c: u32) -> bool`: the ECMAScript WhiteSpace + LineTerminator set — U+0009–000D,
  U+0020, U+00A0, U+1680, U+2000–200A, U+2028, U+2029, U+202F, U+205F, U+3000, U+FEFF — the same
  table as `uo_opc::xml::chars` (copied; a test asserts both agree over every code point).
- `js_trim(s: &str) -> &str` (both ends, that set); `split_js_space(s) -> impl Iterator<&str>`
  (empty tokens skipped, as `split(/\s+/)` + `if (p)`).
- `lower_cp(c: char) -> char` (the liang.ts rule above) and `js_to_lowercase(s: &str) -> String`
  (= `str::to_lowercase`, Final_Sigma included). Both checked against V8 over every code point
  and over the Final_Sigma contexts (Σ after cased letters, with case-ignorable chars between, at
  word end, alone) in the capture (`js` facet); a mismatch is a Unicode-version difference to be
  listed, not fixed by tables.
- `parse_int_hex(units: &[u16]) -> f64`: JS `parseInt(s, 16)` on UTF-16 code units: skip leading
  JS whitespace (a lone surrogate is not whitespace); optional `+`/`-`; optional `0x`/`0X`; then
  hex digits until the first other unit; no digit → `NaN`; result signed.
- `to_uint8(x: f64) -> u8`: ToUint8 (`NaN`, ±∞ → 0; truncate toward zero; modulo 256, negative
  wraps: −1 → 255).

**R-ODTTF (S4)**: `obfuscate`/`deobfuscate` take `&str` and convert it to UTF-16 code units
(`encode_utf16`); the `_utf16` variants take the units from a binding directly (a JS string may
hold lone surrogates). Steps: remove the units `{`, `}`, `-`; `deobfuscate` returns the input
when fewer than 32 units remain; for `i in 0..16` the slice is units `[2i, 2i+2)` clamped to the
length (`slice` semantics: shorter or empty); `key[i] = to_uint8(parse_int_hex(slice))` (a slice
holding a surrogate parses no digit → NaN → 0, as `parseInt`); reverse; XOR the first
`min(32, len)` bytes with `key[i % 16]`. The `odttf` fuzz target and the capture include
non-ASCII and astral keys (`'{é😀…'`).

**R-READS — reads whose value is unused still decide errors (S9).** Every `DataView` read of the
TS is performed (or bounds-checked) by the Rust **in the TS order**, also when its value is not
used, and `&&`/`||`/`?:` short-circuits are kept. Per file:
- `SfntFont` constructor: every directory record's tag, offset and length (all records, in
  order); `req` head, hhea, maxp, OS/2 (missing-table checks before any read of them); then
  `maxp+4` (numGlyphs), `hhea+34`, then `req('hmtx')`; metrics in object-literal order (head+18,
  hhea+4/6/8, OS/2+68/70/72/74/76/62, then `os2.length >= 86 && u16(os2+0) >= 1` → u32 +78,
  `length >= 26` → +24, `>= 4` → +2, `>= 42` → the 10 panose bytes one by one); post +8/+10 only
  when `post.length >= 12`; `parseCmap`: `req('cmap')`, count, then **for every encoding record
  platform, encoding and offset** (all three, whatever the score); then the format; format 4:
  `segX2`, then per segment end, start, delta, rangeOffset (all four, in this order); format 12:
  `nGroups`, then per group start, end, startGlyph.
- `glyphIdsFor` (shared): per record platform and encoding; the offset **only when the score is
  better** (`if (s > score)`), the difference with `parseCmap` that G6 keeps.
- `parseGposKern`: the script-list offset at +4 **is read** although unused (`void
  scriptListOff`); feature-list and lookup-list offsets; every feature record's 4 tag bytes; for
  each kern feature its offset, count, indices; `lookupCount`; per lookup offset, type, subtable
  count, each subtable offset (and for type 9 its type and `u32` offset); PairPos: format,
  coverage offset and the coverage, `vf1`, `vf2` — all **before** testing the format; format 1:
  `pairSetCount`, then per pair set its offset and count, per record `g2` then (only if `vf1 & 4`)
  the XAdvance; format 2: cd1 offset + class def, cd2 offset + class def, `c1n`, `c2n`, the matrix.
- `gposKernInLatin` (inside the same `try`): script count, every script record's tag and offset,
  the feature-list offset at +6 (read first), then as written.
- `parseGsubLigatures`: offsets at +4, +6, +8; every script record; `u16(lookupList)` re-read
  for **every** wanted lookup; per subtable as written (`u16(so)` before the type test of
  format-1 ligatures).
- legacy `kern`: version, count, per subtable length, coverage (both before the format test),
  then pairs.
- subset: `sfntVersion` (`u32` at 0) first, `numTables`, every record (tag bytes by indexing —
  `bytes[p]` past the end is `undefined` → `"\0"`, no error — then offset and length by
  `DataView`, which fail).
- `faceInfo`/`tableOf`: records scanned until the **first** match (records after it are not
  read); OS/2 reads as written, `head` checksum only when `len >= 12`.

**R-BUDGET — the work budget, at named points (S7).** One counter per public call, limit
`max_table_work` (default 2^26). One unit at each point below; when the counter would exceed the
limit the function stops with its own TS catch outcome:

| Function | Counted points | Over the budget |
|---|---|---|
| GPOS parse (`parse_gpos_kern` + `kern_in_latin`) | each coverage entry produced (format 1 entry, each `g` of a format-2 range), each class-def `set`, each class-matrix cell, each `(g1, g2)` pair considered, each format-1 pair record, each kern-feature lookup index | GPOS unreadable (no pairs, no class 0, `kern_in_latin` default) — atomic |
| legacy `kern` | each subtable visited, each pair record | stop; pairs read so far stay (partial, the TS catch) |
| GSUB parse | each feature index appended (repeated tags included), each coverage entry, each single-subst set, each ligature record | GSUB `None` |
| `glyph_ids_for` | each segment/group visited per code point | `Err(Limit(TableWork))` (fails the plan) |
| `standalone_face` | each face scanned × each name record | `Err(Limit(TableWork))` |
| `subset_true_type` | each worklist pop, each component record | the input (`Cow::Borrowed`) |
| `Face::parse` cmap | each range pushed — bounded by `max_cmap_ranges` instead (R-CMAP) | `Err(Limit(CmapRanges))` |
| `glyph_outline` | each `glyph_contours` entry — bounded by `max_outline_work` instead (S1) | `None` |

R-GPOS — pseudo-code for `gpos.rs` (the budget and Map rules; `w` is the work counter):

```text
parse_gpos(view, off, limits) -> Result<Kerning, Unreadable>:
  script_list = off + u16(off+4)?            // read, unused (R-READS)
  feature_list = off + u16(off+6)?; lookup_list = off + u16(off+8)?
  kern_features = [i for i in 0..u16(feature_list)? if tag(feature_list+2+6i)? == "kern"]
  if kern_features empty: return Kerning::empty()          // still followed by kern_in_latin
  lookups = IndexSet()                                     // insertion order, first kept
  for fi in kern_features: fo = feature_list + u16(feature_list+2+6fi+4)?
      for k in 0..u16(fo+2)?: w.tick()?; lookups.insert(u16(fo+4+2k)?)
  count = u16(lookup_list)?
  pairs = PairMap::new(limits.max_kern_pairs); class0 = IndexMap(); class_defs = []
  for li in lookups: if li >= count: continue
      lo = lookup_list + u16(lookup_list+2+2li)?; ty = u16(lo)?
      for s in 0..u16(lo+4)?: so = lo + u16(lo+6+2s)?; t = ty
          if t == 9: t = u16(so+2)?; so = so + u32(so+4)?    // u64 arithmetic
          if t == 2: pair_pos(so)?
  return Kerning { pairs, class0, class_defs, kern_in_latin: kern_in_latin(view, off)? }

pair_pos(o):
  fmt = u16(o)?; cov = coverage(o + u16(o+2)?)?          // Vec<u16>, w.tick() per entry
  vf1 = u16(o+4)?; vf2 = u16(o+6)?; s1 = 2·popcount(vf1); s2 = 2·popcount(vf2)
  if fmt == 1:
    for i in 0..min(u16(o+8)?, cov.len()): ps = o + u16(o+10+2i)?
      for r in 0..u16(ps)?: w.tick()?; rec = ps + 2 + r·(2+s1+s2)
        g2 = u16(rec)?; adv = x_advance(rec+2, vf1)?     // reads only if vf1 & 4
        if adv != 0: pairs.insert_if_absent(cov[i]·65536 + g2, adv)?   // MapFull → Unreadable
  else if fmt == 2:
    cd1 = class_def(o + u16(o+8)?)?; cd2 = class_def(o + u16(o+10)?)?   // w.tick() per set
    c1n = u16(o+12)?; c2n = u16(o+14)?
    m = Vec::new()                                        // no with_capacity(c1n·c2n)
    for c1 in 0..c1n: for c2 in 0..c2n: w.tick()?
        m.push(x_advance(o+16+(c1·c2n+c2)·(s1+s2), vf1)?)
    class1 = IndexMap(); for g in cov: class1.entry(g).or_insert(cd1.get(g).unwrap_or(0))
    adv_of(c1, c2) = if c1 < c1n && c2 < c2n { m[c1·c2n + c2] } else { 0 }
    cd2_id = None
    for (g1, c1) in class1: a = adv_of(c1, 0)
        if a != 0 && !class0.contains(g1):
            cd2_id = cd2_id.or(push cd2 into class_defs); class0.insert(g1, (a, cd2_id))
    for (g1, c1) in class1: for (g2, c2) in cd2 (insertion order): w.tick()?
        a = adv_of(c1, c2); if a != 0: pairs.insert_if_absent(g1·65536 + g2, a)?
// x_advance(p, vf): if vf & 1 { p += 2 }; if vf & 2 { p += 2 }; if vf & 4 { i16(p)? } else { 0 }
// class_def(o): fmt = u16(o)?; IndexMap with last-value-wins, first position kept
// coverage(o): fmt 1 → list; fmt 2 → ranges start..=end; other → empty
// insert_if_absent(k, v): if present: Ok; if len() == max_kern_pairs: Err(MapFull); else insert
// The memory of `m` is c1n·c2n·2 B, bounded by the work budget (2^26 cells → 128 MiB worst case):
// with vf1 & 4 every cell is also bounded by the input (each read is in bounds); without it the
// cells are all 0 and `m` may be replaced by a constant 0 (same results) — required on wasm32.
```

**R-SUBSET — `subset.rs` (S2, S3)** pseudo-code:

```text
enum Off { At(u64), Nan }             // NaN offsets of the TS; At + n, Nan + n = Nan
read_i16/u16(Off::Nan) = the value at offset 0 (DataView ToIndex(NaN) = 0)
cmp(Off::Nan, _) = false; cmp(_, Off::Nan) = false

subset(bytes, used, keep_layout, limits) -> Cow:
  try { ... } catch → Cow::Borrowed(bytes)               // every `?` below
  version = u32(0)?; n = u16(4)?; dir = last-wins map of records (R-READS)
  require head, maxp, loca, glyf else return Borrowed
  num = u16(maxp+4)?; long = u16(head+50)? == 1
  off[i] for i in 0..=num = long ? u32(loca+4i)? : u16(loca+2i)?·2   // eager, in order
  glyph_off(g) = if g <= num { At(glyf + off[g]) } else { Nan }
  keep = {0} ∪ {g in used | g < num}; work = keep as Vec (insertion order), LIFO pop
  while let Some(g) = work.pop():  w.tick()?
    s = glyph_off(g); e = glyph_off(g+1)
    if cmp_le(e, s): continue                             // false when either is Nan
    if i16(s)? >= 0: continue
    p = s + 10
    loop: w.tick()?
      flags = u16(p)?; comp = u16(p+2)?; p = p + 4
      if comp not in keep: keep.insert(comp); work.push(comp)
      p = p + (flags & 1 ? 4 : 2)
      p = p + (flags & 8 ? 2 : flags & 0x40 ? 4 : flags & 0x80 ? 8 : 0)
      if flags & 0x20 == 0: break
      if p is Nan: return Borrowed                        // the TS loops forever here (G11)
  // sizes first (S3): every length in u64, int32 rules of the TS checked
  glyf_len = Σ over g in 0..num with keep(g), len = off[g+1] − off[g] > 0:
      if len + 1 >= 2^31: return Borrowed                 // (len+1) & ~1 is negative in int32
      padded = (len + 1) & !1
  head_len = dir.head.length (declared, u32) ; if head_len < 52: return Borrowed
  other_len(t) = clamp(t.offset .. t.offset + t.length, bytes.len()).len()  // slice clamps
  total = 12 + 16·tables + Σ pad4(len of each kept table)
  if total > limits.max_output_bytes: return Borrowed
  then build exactly as the TS (glyf parts zero-padded, loca long, head copied clamped into a
  zeroed head_len buffer, flags set, tables sorted by tag, checksums wrapping)
```

The parity verifier fuzzes `gpos.rs` and `subset.rs` differentially against the TS (seeded
mutations of the corpus fonts' GPOS and glyf/loca, the TS run in Node over the same files; only
the named G11 outcomes may differ).

**R-CMAP (S8)**: the JS array `push` throws at 2^27 = 134,217,728 elements (measured): a format-12
subtable with more groups rejects the font in `SfntFont` (uncaught) and in the converter
`TtfProgram`. `max_cmap_ranges` (native 2^27 = that limit; wasm32 2^22, i.e. 16 MiB of `order`
indices; real maximum measured: Meiryo 14,335) → `Err(Limit(CmapRanges))` when the
`(n+1)`-th range would be added. Coverage arrays of GPOS/GSUB reach 2^27 only beyond the work
budget (2^26), so the budget covers them.

**R-EMBED (S13, S19)**: `embedding_requests` runs the TS loop up to the face fetch: group by
lowercase family (first spelling), skip `already_embedded` (reason `AlreadyEmbedded`) and
common fonts (`policy.skip_common_fonts` and the lowercased `policy.common_fonts`; reason
`Common`), then emits `(family, variant)` for each wanted variant (`all_variants` → the four
`VARIANTS`, else the used ones, both in `VARIANTS` order). The shell answers each request with
`documentFace ?? (own file ? faces[v] ?? loadFace)` — exactly the calls the TS makes, no
prefetch. `plan_font_embedding` repeats the same walk with the answers: `found`/`blocked`,
`is_embeddable`, subset with `glyph_ids_for(..)?` (its `Err` and `face_info`'s `Err` fail the
plan, as a TS throw rejects `planFontEmbedding`), `subsetted = subset && has_true_type_outlines`
(S11b), skipped reasons in the TS order. `!policy.embed` → empty requests and an empty plan. The
TS `app` maps to a policy: Word = `{common: WORD_COMMON_FONTS, all_variants: false,
skip_common_fonts: settings.skip_common_fonts}`, PowerPoint = `{POWERPOINT_COMMON_FONTS, true,
true}` — written in the format crates / shell, not in uo-fonts.

**S11 items, written in**: (a) `want_postscript: Some("")` = `None`; (b) `subsetted` as above;
(c) `HyphenationRegistry::register` replaces an existing id; (d) the `dir` capture facet is the
TS `Map` order — first insertion position of each tag, last value — not file order; (e) `kern`
facet keys above `2^32` (overflow keys) are compared as keys only (Rust keeps no value for them);
(f) `Face` keeps a copy of its `FontLimits`, and `FontLimits::DEFAULT` is chosen by
`target_pointer_width`; (g) `ColrInfo` offsets are `u64`; (h) `class_defs` keeps only the cd2
maps referenced by class 0; (i) class-def keys go up to 131,069.

**`FontProgram::legacy_kern` (S18, G16)**: the C# `TtfProgram.ParseKern` exactly — parsed lazily
(`OnceLock`), **no version check** (it reads `nTables` at +2), subtables with format 0,
horizontal, not cross-stream, `p += length`, every pair `pairs[(left << 16) | right] = value`
(**last wins**), any read failure clears everything; bounded by `max_kern_pairs` (over it:
cleared, the C# catch) and the work budget. Parity source: a small C# test
(`tests/parity_support/legacy_kern.cs`, run with `dotnet run`) emitting the same `uo-testkit`
stream format over the corpus and the quirk fonts → `tests/parity/legacy_kern.cap`.

### Parity plan

Capture from the frozen TS with `uo-testkit` (`js/testkit.mjs`, `UO_TS_ROOT` = a built checkout
of `main`): `core/crates/uo-fonts/tests/parity_support/capture_fonts.mjs` →
`tests/parity/fonts.cap` (hashed canonical streams, `@ts-commit`, `@node` version and platform in
the meta). Replay in `cargo test` (`tests/fonts_parity.rs`, shares like `opc_parity`). The TS
private fields (`kernMap`, `kernClass0`, `tables`, `cmapRanges`) are read directly by the capture
script (TS `private` is compile-time only). Captures hold digests only — **never font bytes**.

Facets per font input (keys: corpus path + face index):

| Facet | Captured from | Stream |
|---|---|---|
| `open` | `new SfntFont` | ok, or the error class (`missing:TAG`, `cmap-none`, `cmap-format:N`, `range`) |
| `dir` | `tables` | records (tag, offset, length) in TS `Map` order: first insertion position of each tag, last value (revised after the skeptic, S11d) |
| `metrics` | `font.metrics` | every field, optional ones with presence, panose bytes |
| `cmap` | `glyphId(cp)` for **every** cp in `0..=0x10FFFF` (+ `0x110000`, `0xFFFFFFFF`) | (cp, gid) where gid ≠ 0, as f64 (catches saturation); TS throws → `E` (G4) |
| `adv` | `advanceWidth(g)` for `g in 0..numGlyphs + 16` and `0xFFFF`, `0x10000`, `2^32` | value or `E` |
| `kern` | `kernMap` (sorted by key) + `kernClass0` (g1, adv, digest of its cd2 entries in insertion order) + `kernInLatin`; `kernGlyphsTwips` at sizes {6, 10.5, 11, 12, 72} over every stored pair, 1,000 class-0 pairs and 10,000 seeded random pairs | Rust replays the units with a test copy of the twips formula; keys ≥ 2^32 (the `overflow` set) compared as keys only (S11e) |
| `gsub` | features (tag → lookup list), lookups and singles digests; `ligate(gids, feats)` for every ligature sequence of the font, each with feature sets `[liga]`, `[liga, clig]`, `[liga, clig, dlig, hlig]`, every single-subst feature, and the corpus words | groups or `null` |
| `vdmx` | `vdmx(p)` for `p in 0..=300` and `p + 0.5` | entry or none |
| `colr` | `hasColorGlyphs`, `colorLayers(g)` for every gid | layers (gid, color, alpha bits) / `null` / `E` |
| `outline` | `glyphOutline(g)` for every gid (+ out of range) | command stream, f64 bits |
| `symbol` | `isSymbolFont` | bool |
| `program` | converter `TtfProgram`: open outcome, bbox, ascender, descender, italicAngle bits, glyphId over every cp, advanceWidth | |
| `info` | `isFontFile`, `isEmbeddable`, `hasTrueTypeOutlines`, `faceInfo` (names as code units), `glyphIdsFor` over the corpus code points and 100 seeded sets | |
| `subset` | `subsetTrueType(bytes, ids, {keepLayout})` for: all ids, the corpus text's ids, {0}, 200 seeded sets, ids ≥ numGlyphs, both `keepLayout` values | output digest (byte-exact) |
| `ttc` | `standaloneFace` of generated collections (open faces), each PostScript name, a missing one, none | output digest or error |
| `eot` | `sfntToEot(bytes, {subsetted})` both | digest |
| `odttf` | `obfuscateOdttf` / `deobfuscateFont` over GUIDs: valid, lower case, no braces, short, non-hex, signs, spaces, `0x`, *(revised after the skeptic, S4)* non-ASCII, astral and lone-surrogate keys (passed to Rust as code units) | digest |
| `legacy_kern` *(S18)* | the C# `TtfProgram.KernAdjust` over every stored pair + 10,000 seeded pairs, `HasKerning` — captured by `legacy_kern.cs`, same stream format | `legacy_kern.cap` |

Other facets:
- *(revised after the skeptic, S15)* `js`: `lowerCp` and full-string `toLowerCase` over every code
  point and the Final_Sigma contexts; `parseInt(s, 16)` over 2-unit strings (all ASCII pairs,
  surrogates, whitespace, signs); `split(/\s+/)` over the JS whitespace set.
- *(revised after the skeptic, G11)* the differential fuzz of `gpos.rs` and `subset.rs`: seeded
  mutations of the corpus fonts' GPOS and glyf/loca, the TS run in Node on the same files; only
  the named G11 outcomes may differ.
- `hyphen`: per language, every `HyphenationPatterns` field digest and the NFC check; `points`
  and `hyphenate(word, l, r)` for l, r in {1, 2, 3} over: every letter run of the oracle corpus
  text (the 2,742 docx `w:t`, NFC per cluster as the layout does), every exception word, words
  made from the patterns' letters (seeded), and casing variants; `hyphenationPatternId` over a
  list of 300 language tags (case, `_`, prototype names); `lowerCp` over every code point.
- `embed`: `planFontEmbedding` with a stub resolver (deterministic sources: own file, document
  face, substitute, unavailable, restricted fsType 0x2, bitmap-only 0x200, CFF, a malformed cmap
  that throws) × settings × app × usages (repeated families with different case) → *(revised
  after the skeptic, S13)* the **sequence of `documentFace`/`loadFace` calls** the stub saw
  (compared with `embedding_requests`), then faces (family, variant, bytes digest, subsetted,
  info digest), skipped, or the rejection.
- `ooxml`: *(revised after the skeptic, S17)* moved with the writers to the uo-docx/uo-pptx
  plans: read and write over the oracle docx, the slides samples and a generated XML quirk
  corpus, `crypto.getRandomValues` stubbed to a seeded sequence.

Corpus (no proprietary font file is ever written to the repository):
- Open fonts in the repository: `shared/fonts/files/*.ttf` (16), Liberation Sans (4),
  Amatic SC (2); the WOFF2 files and the Chivo EOT as negative inputs (`open` facet only).
- Fonts embedded in committed documents, extracted **in memory** at capture and replay time:
  `site/assets/samples/demo.docx` (Ubuntu ×4, Ubuntu Mono, Tahoma — embedding allowed, listed in
  `tools/fonts/allowed-fonts.txt`) and `emwidth/ew-emb.docx` (Amatic SC as "Wfit Test"). Only 2
  of the 2,800 committed docx/pptx embed fonts.
- Generated quirk fonts (`examples/font_quirk_corpus.rs` over `uo_testkit::generate::sfnt`, built
  from the open fonts' tables plus synthetic tables): cmap subtable orders and formats (4, 12,
  6, 14, odd `segX2`, overlapping/unsorted segments, (3,0) only, (3,0) + (1,0)), duplicate tags,
  short OS/2 of each version, `numberOfHMetrics` 0, hmtx at offset 0, GPOS format 1/2 with class 0,
  g2 class 0 listed, classdef format 1 with `start + i > 0xFFFF` (key collisions), extension
  lookups, coverage format 3, repeated lookups across features, legacy kern with failure
  mid-table and `length` 0, a font at the `Map` limit (one case, `#[ignore]`d in debug), GSUB
  single last-wins, lookups with both types, ligatures with `cc` 0/1, COLR/CPAL with palette
  edge cases, VDMX ratios and duplicate ppem, composites (each scale form, depth 9, self
  reference, 64+ components, end ≥ nPts), loca short/long, truncated tables everywhere, `.ttc`
  with 1, 4 and hostile `numFonts`, a directory multiplying the file.
- Opt-in (`UO_LOCAL_FONTS=1`, maintainer machine, before gate 2): the fonts copied by `npm run
  fonts:sync` and the Windows/Office cloud fonts (Calibri, Cambria, Aptos, Segoe UI, Gabriola,
  Yu Gothic, Meiryo, Microsoft YaHei, Segoe UI Emoji…): a separate, git-ignored capture — the files
  exist only there. *(revised after the skeptic, S5)* It covers large kerning maps (Gabriola
  742,163 pairs), large cmaps (Meiryo 14,335 ranges), VDMX (218 Windows faces) and one COLR font
  (Segoe UI Emoji); it does **not** cover class-0 kerning (0 entries on all 472 Windows, 148
  cloud and 22 repository faces): **class 0 is covered only by generated fonts**, which the
  generator builds from each open font's real coverage and class defs (synthetic tables,
  committable); the TS runs on them, so parity stays exact. Gate 2 states this.

Comparison: exact — f64 by bits (−0 and NaN included), bytes by digest, maps in a canonical order
(sorted keys) plus the insertion-order facets where order is observable (class 0 order, cd2 order,
ligature lists, embedding requests). Accepted differences only by name: G4 (lazy reads), G5 and
G11 (limits and hostile-input outcomes), G6 (unifications, none during parity), G9 (own keys),
G12 (none expected: code units reproduce the TS).

### Memory plan (measured gate)

- **M1** (`assert_no_leak`): `Face::parse` (ok and each error class), every query (first call,
  which parses lazily, and later calls), `FontProgram::parse`/queries, `glyph_ids_for`,
  `face_info`, `standalone_face`, `subset_true_type` (both modes, failure path), `sfnt_to_eot`,
  `odttf`, `Hyphenator::new`/`points`/`hyphenate`, `HyphenSession` (revised after the skeptic),
  `embedding_requests` and `plan_font_embedding` (ok and rejected), and the limit paths
  (`max_kern_pairs` reached, work budget hit at each R-BUDGET point, `max_outline_work`,
  `max_cmap_ranges`, `max_output_bytes`, the subset `Nan` loop exit). Caches are dropped with
  their owner; `clear_caches()` is a no-op.
- **M2 soak**, two scenarios:
  - *fresh* (tolerance **0 B**): 1,000 × (parse the corpus faces (open + the 6 in-memory embedded)
    → measure the oracle text code points with kerning at 11 pt → ligate 200 words → 200
    outlines → COLR and VDMX queries → subset both modes → EOT → odttf round trip → `face_info`
    → `.ttc` extraction → hyphenate 10,000 words in the 7 languages with fresh sessions →
    embedding requests + plan → drop everything), sampled every 50.
  - *long-lived engine*: the same with faces, hyphenators and caches kept across iterations; after
    warm-up the series stays within the sum of the cache caps (outline caches + hyphen caches),
    and returns to the start after dropping the engine.
  - wasm: the same scenarios behind a test-only `uo-wasm` feature (`fonts-memtest`), with
    `memory.buffer.byteLength` flat after warm-up; WebKit and real Safari 15 before gate 3.
- **M3 budgets** (recorded in stage 4, +10% rule). References measured now (Node 24.15, frozen
  dist, `--expose-gc`): `SfntFont` of Carlito Bold retains **≈ 7.1 MB** of heap (161,520 kerning
  pairs), parse 21–36 ms; Caladea ≈ 3,000–3,500 pairs, ≈ 0.3 MB; Liberation Serif 821–867 pairs;
  Liberation Mono 0. Spike: 26 fonts parsed 10× in 33 ms native / 42 ms wasm. Targets: Carlito
  `Face` kept ≤ 2.5 MB (pair map 171,859 entries × 8 B buckets) and parse + first kern ≤ 3 ms
  native; fonts without GPOS ≤ 64 KB kept beyond the shared bytes (holds for CJK only with the
  compact cmap: Meiryo 14,335 ranges × 4 B = 57 KB; revised after the skeptic, S16); de-1996
  hyphenator ≤ 4 MB kept (TS reference to measure in stage 4); subset of Carlito to the oracle
  text: peak ≤ 2 × output + loca.
  *(revised after the skeptic, S16)* Opt-in budgets (local fonts, recorded but not in CI):
  Gabriola (742,163 pairs: ≈ 2^20 buckets × 9 B ≈ 9.4 MB per face with the hash `PairMap`),
  Calibri × 4 (≈ 1.1 M pairs), Meiryo (cmap). In the browser the font bytes live twice (the
  JS `Uint8Array` the `FontFace` needs and the wasm copy): recorded as part of the budget;
  `standalone_face` output becomes `Arc<[u8]>` without a second copy. The frozen sorted pair
  arrays (`Box<[u32]>` + `Box<[i16]>`, 6 B per pair) and class-based kerning are stage-5
  candidates.
- **M4 boundaries**: `Face`, `FontProgram`, `Hyphenator`, `HyphenSession` handles (10,000 create/
  free, explicit and finalizer paths) when the bindings arrive; in this module, the test-only
  wasm export takes byte buffers and returns numbers (as uo-opc's `opc-memtest`).
- **M5 caches**: outline cache per face (`outline_cache_entries`, filled past it with distinct
  glyphs: bytes bounded), the `HyphenSession` cache (50,001st entry clears), the lazily parsed
  tables (bounded by the font and `max_table_work`), `PairMap` (`max_kern_pairs`), the compact
  cmap (`max_cmap_ranges`). The TS advance cache and GPOS `WeakMap` do not exist in Rust; the
  capped `FaceCache` of the native provider is tested in uo-docx-layout (S16).

### Security plan

Untrusted inputs: font bytes (embedded in documents as odttf/EOT, from hosts, Local Font Access,
`.ttc` collections), font keys (`w:fontKey`), OOXML parts (fontTable, settings, presentation,
rels, content types), document text (usage, hyphenation words). Trusted: the shipped pattern
files and catalog.

Limits (`FontLimits`; native / wasm32 defaults, G5, revised after the skeptic):
`max_kern_pairs` 2^24 / 2^22 (measured real maximum 742,163: 5.6× margin on wasm32, G5 final),
`max_table_work` 2^26 / 2^26 (measured real maximum 6.9·10^6: 9.7×), `max_outline_work` 4,096,
`max_outline_points` 2^20, `outline_cache_entries` 2,048, `max_cmap_ranges` 2^27 / 2^22,
`max_output_bytes` 256 MiB / 64 MiB (`standalone_face` and `subset_true_type`). No allocation
from a declared count (cmap groups, pair sets, class matrices, directory sizes, loca lengths,
`head.length`): vectors grow with what is read, and output sizes are computed and checked before
allocating (the spike's `Vec::with_capacity(c1n * c2n)` is the counter-example). Every
`u64 → usize` is `try_from` (wasm32). No recursion except the outline (depth ≤ 8, the TS bound,
and `max_outline_work` visits).

Fuzz targets (`core/fuzz`, plain `fn(&[u8])`, libFuzzer + the stable fallback driver):
`font_parse` (parse + every query over derived cps/gids/sizes/features), `gpos_kern`
(arbitrary GPOS inside a valid font; invariants: lookups never panic, `kern_units` equals a
naive model built from the same expansion), `gsub_ligate` (groups cover the input, `len` sums
match), `glyph_outline`, `colr_vdmx`, `font_program`, `info_ttc` (`face_info`,
`standalone_face`: output parses as a table directory, within the cap), `subset` (output re-parses
when the input parses with glyf; kept glyph bytes equal the input's), `eot_write`, `odttf`
(round trip; key parsing against a model of `parseInt`), `hyphen_build` + `hyphen_points`
(arbitrary patterns/exceptions/words: positions in `1..n`, sorted, distinct), `embed_plan`
(requests then plan: the plan asks only for requested faces). *(revised after the skeptic)*
`ooxml_*` move with the writers; every target runs with small limits and asserts that each
public call finishes within a time bound (catches a missed budget point). Seeds: the corpus
fonts, their tables, the quirk corpus.

### Risks

- *(revised after the skeptic)* The `ooxml` regex semantics leave this module with the writers
  (G2); they remain the main risk of the uo-docx/uo-pptx plans.
- `gpos.rs` and `subset.rs` carry the budget, Map and NaN rules: pseudo-code above and a
  differential fuzz against the TS.
- Unicode version: V8 (ICU) vs Rust std lowercase — checked over all code points; patterns' NFC
  checked in the capture.
- No open font in the repository exercises class-0 kerning, COLR or VDMX: class 0 only by
  generated fonts (no real font has it, S5); COLR by generated fonts + Segoe UI Emoji opt-in;
  VDMX opt-in. Gate 2 states it.
- The `Map` limit (2^24) depends on V8: the capture records the Node version; a test asserts the
  measured limit when capturing.
- Memory of expanded kerning (TS 7.1 MB, Rust ≈ 2.4 MB per Carlito face) stays high until the
  stage-5 class-based representation (spike lesson 3).

### Sonnet readiness (revised after the skeptic)

Re-assessed per module after S1–S19. "Closed" = every decision the translator would face is
written above (types, signatures, catch points, read order, budget points and their outcomes,
Map/order/UTF-16 rules, parity facets).

| # | Module | Status | What closed it (beyond the first plan) | Port order / note |
|---|---|---|---|---|
| 0 | `js` | closed | R-JS (S4, S15) | first; its V8 facet must pass before the rest replays |
| 1 | `read` | closed | `Off::Nan` belongs to subset only | |
| 2 | `limits`, `error` | closed | S1, S8, S11f fields | |
| 3 | `sfnt` (directory) | closed | R-READS (directory reads), S11d facet | |
| 4 | `cmap` | closed | compact layout (S16), R-READS order, R-CMAP (S8) | three named readers (G6) |
| 5 | `face`, `vdmx`, `colr` | closed | R-READS constructor order, S11g | |
| 6 | `gpos` | closed, **highest risk** | R-GPOS pseudo-code, R-BUDGET, S11h/S11i | differential fuzz vs TS |
| 7 | `gsub` | closed | R-BUDGET points (S7), R-READS | |
| 8 | `outline` | closed | `max_outline_work` (S1) | |
| 9 | `program` | closed | `legacy_kern` = the C# rules, C# capture (S18) | |
| 10 | `info` | closed | panose clamp (S10), first-match reads | |
| 11 | `ttc` | closed | S11a, budget, output cap | |
| 12 | `subset` | closed, **highest risk** | R-SUBSET pseudo-code (S2, S3) | differential fuzz vs TS |
| 13 | `eot` (write) | closed | clamped panose bytes (S10) | |
| 14 | `odttf` | closed | R-ODTTF code units (S4) | |
| 15 | `variant`, `catalog` | closed | `js_trim`/`js_to_lowercase` | |
| 16 | `hyphen` | closed | `HyphenSession` (S12), `register` replaces (S11c) | |
| 17 | `embed` | closed | R-EMBED two phases, app-neutral policy (S13, S19) | |
| — | `ooxml` | not in this module | deferred (G2, S17); Opus in the format-crate plans | |
| — | `eot::read`, `system`, `symbols` | deferred (G3) | own plans when scheduled | |

With G2 = defer, every module of uo-fonts is a Sonnet translation under the parity and memory
verifiers; the orchestrator should move `gpos` or `subset` to Opus if the parity verifier sends
the same issue back twice (skill rule).

## Questions for the maintainer (gate 1)

**Approved 2026-11-01.** Maintainer, verbatim: "Eu aprovo tudo" — every final recommendation accepted.

*(revised after the skeptic)* One consolidated list: the planner's G1–G10 and the skeptic's
G11–G16, with the skeptic's revision folded in and one **final recommendation** per question.

| id | Decision | Planner | Skeptic | Final recommendation |
|---|---|---|---|---|
| G1 | Scope: uo-fonts = font files (sfnt, cmap, metrics, GPOS kerning, GSUB ligatures, outlines, COLR, VDMX, converter program, subset, `.ttc`, EOT write, odttf, face info, variants, catalog, embedding plan, hyphenation); the 600-DPI measuring model, `FaceResolver`, `faceMetrics`, line height, `hyphenate.ts` and `math.ts` go to uo-docx-layout; `EmbeddedFont` to uo-pdf; the embedded-font readers and Slides' `sfntStyle`/`looksLikeSfnt` to the format crates; the third deobfuscate copy is replaced | approve | approve + S14 users | **approve** (S14 rows are in "Left for other modules") |
| G2 | The docx/pptx embedding writers (`ooxml.ts`): (a) **defer** module 18 to the uo-docx and uo-pptx plans, which own `fontTable.xml`/`settings.xml`/`presentation.xml` (the spec table stays in this record for them); (b) keep in uo-fonts as a documented post-pass over the saved `uo_opc::Package` (feature `ooxml`, Opus) | (b) | (b) only as a post-pass, or (a) | **(a) defer** — one owner per part, no uo-opc dependency and no Opus module in uo-fonts; the decision of *what* to embed stays single (`embed`) |
| G3 | Defer `eot::read` (MTX), `system` (native font index), `symbols` (Slides' name-based symbol fonts); slots reserved | approve | approve | **approve** |
| G4 | Reads the TS does not catch (format-4 glyphIdArray, hmtx, `colorLayers`, outline points past `nPts`) become total functions (0 / `None`), accepted by name; `face_info`, `standalone_face` and `glyph_ids_for` return `Result` and a failure rejects the embedding plan as in the TS | approve (with `glyph_ids_for` partial) | `glyph_ids_for` → `Result` | **approve as revised** (no partial results anywhere) |
| G5 | Limits: `max_kern_pairs` 2^24 native (the measured V8 `Map` limit, exact parity) / 2^21 wasm32; `max_table_work` 2^26 at the R-BUDGET points; `max_outline_points` 2^20; `max_output_bytes` 256/64 MiB; over a limit, each function's own TS catch outcome | approve | approve with measured numbers: 742,163 pairs (Gabriola) → 2.8× margin on wasm32 (or 2^22 for ≥ 5×), work 6.9·10^6 → 9.7× | **approve; wasm32 pair cap 2^22** (≈ 75 MB worst case, 5.6× the largest real font) — 2^21 left too little margin for unmeasured CJK/Office fonts |
| G6 | Disagreeing readers (three cmap readers, last- vs first-wins directories, TS vs C# legacy kern) kept as named readers during parity; unification proposed in stage 5 with evidence | approve | approve | **approve** |
| G7 | Hyphenation data generated once into `data/hyphen/<id>.txt` (NFC checked against V8), not in the wasm, feature `hyphen-data` for native/.NET; registry per engine, cache in a `HyphenSession` bound to its hyphenator (S12) | approve | approve + S12 | **approve as revised** |
| G8 | JS helpers: (a) depend on uo-jsnum, gated on its gate 2; (b) a private `js` module in uo-fonts (R-JS: JS whitespace, trim, `lower_cp`, `js_to_lowercase`, code-unit hex `parseInt`, `to_uint8`), moved to uo-jsnum after its gate 2 | (a) | (b) — uo-jsnum's plan has no lowercase helper and a `&str`-only `parse_int` | **(b)**; and correct the uo-fonts line of `uo-jsnum.md` |
| G9 | Catalog generated once from `catalog.ts` and checked in (re-parse test while the TS exists); shell and .NET read it from the core; own keys only (prototype hits fixed, accepted by name) | approve | approve | **approve** |
| G10 | Models: stage 3 on Sonnet | Sonnet 1–17, Opus `ooxml` | approve after S1–S4, S7–S13, S18 are written in | **Sonnet for modules 0–17** (all now closed, see "Sonnet readiness"); `gpos`/`subset` with the pseudo-code and the differential fuzz; Opus for `ooxml` in the format-crate plans (G2a) |
| G11 | Hostile inputs where the TS hangs, aborts or allocates without bound get a Rust outcome, accepted by name: composite outline visits over `max_outline_work` → `None` (S1); subset `Nan` component loop → the input (S2); subset output over `max_output_bytes` or an int32-negative length → the input (S3); cmap ranges over `max_cmap_ranges` → font rejected (S8); every R-BUDGET point | (new) | approve | **approve** |
| G12 | odttf keys processed as UTF-16 code units; a surrogate in a slice → 0 (S4) | (new) | approve | **approve** (reproduces the TS; no accepted difference) |
| G13 | Embedding API: two phases (`embedding_requests` → the shell fetches exactly those faces → `plan_font_embedding`), app-neutral `EmbeddingPolicy` (`common_fonts`, `all_variants`), the Word/PowerPoint tables as consts (S13, S19) | (sync source + app enum) | approve | **approve** |
| G14 | Memory: compact cmap now; hash `PairMap` with opt-in M3 budgets for Gabriola, Calibri × 4, Meiryo; sorted pair arrays / class-based kerning as stage-5 candidates; a capped `FaceCache` in the Rust layout provider (S16) | (expanded map; dedupe in bindings) | approve | **approve** |
| G15 | `math.ts` `parseFont` to uo-docx-layout now, to uo-fonts (`Face::math()`) when Slides equations need it (S14) | (missed) | approve | **approve** |
| G16 | `FontProgram::legacy_kern` (C#-only) kept, verified by a C# capture in the same stream format (S18); the alternative is to drop it from 0.10 until the C# exporters are ported | (no source) | approve the C# capture | **approve the C# capture** (small: one test file; the converter's DocxPdf kerning depends on it) |

## Analysis skeptic (verified in the code)

Stage 2, Opus, 2026-11-01. Every claim below was checked in the frozen TS/C# or measured. The
measurements ran a probe (outside the repository) over the frozen `main` dist (`SfntFont`
private fields read directly) on 472 faces of `C:\Windows\Fonts`, 148 Office cloud fonts
(`%LOCALAPPDATA%\Microsoft\FontCache\4\CloudFonts`: Aptos, Montserrat, Meiryo…) and the 22 repo
fonts; no font bytes were copied anywhere. V8 limits measured on Node 24.15.0.

| # | Finding (evidence) | Blocking | Proposed decision |
|---|---|---|---|
| S1 | **Composite outlines are exponential in work, not in points.** Up to 64 components per level (sfnt.ts:477) and depth ≤ 8 (sfnt.ts:429): a glyph whose 64 components are itself makes ≈ 64^9 ≈ 1.8·10^16 calls and accumulates **zero** points, so `max_outline_points` (plan, limits) never trips; the TS hangs. Reached from painting and PDF (canvas.ts:358, pdf.ts:273, textfx.ts:116, engine.ts:4167) with an embedded font. | **yes** | `max_outline_work` (component visits per `glyph_outline` call, counted at every `glyphContours` entry; default 4,096); over it → `None` (cached). Accepted by name (TS hangs). G11. |
| S2 | **The subset can loop forever; the plan says it cannot.** For a component id > `numGlyphs`, `s = glyf.offset + undefined = NaN` (subset.ts:57); `getInt16(NaN)` reads offset 0 and `NaN + 10` stays NaN (measured: `getUint16(NaN + 10)` = bytes 0–1), so when the first u16 of the file is negative with bit 0x20 (e.g. `FF E1`) the component loop (subset.ts:61–71) re-reads offset 0 forever. Neither `subsetTrueType` nor `SfntFont` checks the sfnt version (subset.ts:34, sfnt.ts:73). The plan's "no count guard (it ends at the first read past the input)" is false on this path; a faithful port hangs. | **yes** | Model the NaN offset as an explicit state (`Off::Nan` → reads at 0, `+ n` stays `Nan`, comparisons false); a component loop in the `Nan` state with bit 0x20 → return the input (`Cow::Borrowed`, the TS catch outcome). Plus the work budget of S7 on the component loops. G11. |
| S3 | **The subset allocates from declared sizes.** `new Uint8Array(padded)` with `padded = (len + 1) & ~1` from loca offsets (subset.ts:83–87; long loca is any u32, unrelated to the file size; `& ~1` is int32, so `len ≥ 2^31 − 1` throws) and `new Uint8Array(head.length)` from the directory (subset.ts:100). The plan (subset section: "clamped … missing bytes stay 0") asks to reproduce this byte for byte → attacker-chosen GB allocations, an abort on wasm (the R5 lesson of uo-opc). `subset_true_type` has no `FontLimits` parameter. | **yes** | Add `limits: &FontLimits`; compute the output size first (checked: Σ padded + `head.length` + other clamped tables + directory); over `max_output_bytes`, or a negative int32 `padded`, → return the input (the TS catch outcome, and its allocation failure). Zero padding below the cap stays byte-exact. G11. |
| S4 | **odttf keys are parsed in UTF-16 code units.** `hex.length < 32` and `hex.slice(2i, 2i + 2)` (docx-core fonts.ts:30–33; ooxml.ts:159–161; slides-core fonts.ts:70–74) count code units of an untrusted attribute. Measured: `'{é😀'` → `length` 4, `slice(1, 3)` splits the surrogate pair, `parseInt` → NaN → 0. A translation over `&str` byte indices either panics on a char boundary or changes the `< 32` decision. The plan's odttf spec does not say. uo-jsnum's planned `parse_int(s: &str, …)` cannot take a lone surrogate. | **yes** | `odttf::{obfuscate, deobfuscate}` take `&str`, remove `{`, `}`, `-`, convert to `Vec<u16>`; the length check and the 2-unit slices are in code units; a slice holding a surrogate → 0 (what `parseInt` gives), else `parse_int` on the 2 chars and `to_uint8`. Non-ASCII keys in the `odttf` fuzz target and the capture. G12. |
| S5 | **The opt-in capture does not cover class-0 kerning.** Measured `kernClass0.size` = 0 on all 472 Windows faces (Calibri, Cambria, Segoe UI, Yu Gothic, Microsoft YaHei, Gabriola…), all 148 cloud fonts and all 22 repo fonts. COLR: one face (Segoe UI Emoji). VDMX: 218 Windows faces (well covered), 0 in the repo. The plan (corpus, risks) says the local run covers class 0. | no | Class 0 is covered only by generated fonts — still exact parity, because the capture runs the TS on them. The generator must emit class-0 GPOS built from each open font's real coverage and class defs (synthetic tables are committable). Gate 2 states: class 0 generated-only; COLR generated + Segoe UI Emoji opt-in; VDMX opt-in. |
| S6 | **Limits measured on real fonts.** Largest pair map: Gabriola 742,163 (Montserrat 537,176; Calibri Light 535,562; Calibri 346,926). Largest GPOS work (coverage + class defs + class matrix + pair expansion): Gabriola 6.9·10^6; parse 177 ms in TS. So the wasm32 cap 2^21 has a 2.8× margin (not the 10× of G5) and `max_table_work` 2^26 has 9.7×. V8 `Map` limit re-measured: exactly 2^24, the 2^24+1-th insertion throws. | no | Keep 2^21 on wasm32 (every measured face fits) or take 2^22 for ≥ 5×; record these numbers; `MapFull` only for an absent key when `len() == cap`. G5 revised. |
| S7 | **The work budget covers only GPOS/GSUB.** Other loops are unbounded on small hostile input: legacy `kern` with `length` 0 re-reads the same subtable (sfnt.ts:178–193: 65,535 × 65,535 `has` checks); `glyphIdsFor` scans every segment/group per code point (shared sfnt.ts:76–93: code points × groups); `standaloneFace` scans `numFonts` × name records (shared sfnt.ts:186–189, 160–170); the subset component loops (S2); GSUB appends a feature's lookup list per repeated index (gsub.ts:44–49: 65,535 × 65,535). | no | One `max_table_work` budget per call, counted at named points (list them in the plan); over it, the outcome of that function's TS catch: legacy kern partial, `glyph_ids_for`/`standalone_face` `Err`, subset input, GSUB `None`, GPOS unreadable. |
| S8 | **JS array length limit missing from the trap list.** `push` throws `RangeError: Invalid array length` at 2^27 = 134,217,728 elements (measured, default heap). Reachable: coverage format 2 with ≥ 2,048 full ranges (gpos.ts:104–110, gsub.ts:54–57; caught) and cmap format-12 groups (sfnt.ts:287–294; uncaught → font rejected). A Rust `CmapRange` costs ≈ 32 B per 12-byte group. | no | Add to the trap list; `max_cmap_ranges` in `FontLimits` (2^27 native = the TS rejection, lower on wasm32) → `Err(Limit)`; same for `FontProgram`. |
| S9 | **Reads whose value is unused still decide errors.** `parseCmap` reads every record's offset (sfnt.ts:249) while `glyphIdsFor` reads it only on a better score (shared sfnt.ts:67); the subset reads `sfntVersion` first (subset.ts:34); the constructor reads `numGlyphs`/`numberOfHMetrics` before `req('hmtx')` (sfnt.ts:84–86). Unifying the readers or skipping "useless" reads changes which fonts open. | no | Rule in the trap list: every `DataView` read is performed (or bounds-checked) in the TS order, also when its value is unused; `||`/`&&` short-circuit order kept. |
| S10 | **`faceInfo` panose is clamped, not 20 chars.** `bytes.subarray(os2.off + 32, os2.off + 42)` (shared sfnt.ts:123) clamps at the end of the file: with `os2.len ≥ 42` and a table cut by EOF the panose is a shorter (possibly empty) hex string, written to `w:panose1` and to the EOT (eot.ts:24: `''` → NaN → 0). | no | Name the clamp; `FaceInfo::panose` keeps the short string; EOT panose bytes = hex pairs present, the rest 0 (no general `parseInt` needed there). |
| S11 | **Small gaps that would force a Sonnet decision:** (a) `standaloneFace` tests `if (wantPostscript)` (shared sfnt.ts:185): `Some("")` = `None`; (b) `subsetted` is `settings.subset && hasTrueTypeOutlines` even when the subset fell back to the input (embedding.ts:126–128); (c) `register` replaces an existing id (index.ts:51–54, `Map.set`); (d) the `dir` facet order is the first insertion of each tag with the last value (Map), not file order; (e) `kern` facet keys ≥ 2^32 compared by key only (Rust keeps no value); (f) `Face` keeps a copy of its `FontLimits` (lazy GPOS needs them) and `FontLimits::DEFAULT` = by target pointer width; (g) `ColrInfo` offsets are sums of u32 (sfnt.ts:316–318): `u64`; (h) `class_defs` keeps only the cd2 referenced by class 0 (the TS frees the others); (i) the claim "keys up to 131,070" is 131,069 (65,535 + 65,534). | no | Write (a)–(i) into the plan. |
| S12 | **`HyphenCache` is not bound to its hyphenator.** The TS cache belongs to the instance (liang.ts:42) and is keyed by the word only; a caller-owned cache (plan API) passed to another language's hyphenator returns the wrong points. | no | `HyphenCache` records the hyphenator id and clears on mismatch (or `HyphenSession { Arc<Hyphenator>, cache }`). |
| S13 | **The synchronous `EmbeddingSource` changes what the shell must fetch.** The TS awaits `loadFace` only for families past the already-embedded/common checks and only for the wanted variants (embedding.ts:113–122). A sync source makes the shell prefetch every variant of every family (Slides: 4, common fonts included) or duplicate the skip rules in TS. Also: in the TS a `glyphIdsFor`/`faceInfo` throw rejects the whole plan; G4 makes `glyph_ids_for` partial, which would silently embed an incomplete subset. | no | Two-phase API: `embedding_requests(usage, settings, app, already) -> Vec<(String, Variant)>`, then `plan_font_embedding(…, faces)`; `glyph_ids_for` returns `Result` and the plan propagates `Err` like the TS. G13. |
| S14 | **Missed users.** `docx-layout/src/math.ts:59–98` `parseFont`: a fourth directory reader (last-wins Map, `head` only required, `unitsPerEm \|\| 2048`) for MATH constants, italic correction (coverage) and a glyph ink box with its own bounds check, cached in a `WeakMap`. slides-core `fonts.ts:37–66` `sfntStyle`/`looksLikeSfnt` (own OS/2 and signature rules: no WOFF, ≥ 4 bytes) and a third `deobfuscateFont` copy (69–80). | no | List them in "Left for other modules": math.ts → uo-docx-layout (or `Face::math()` here if Slides equations need it — G15); slides helpers → uo-pptx over `face_info`/`is_font_file`, differences named; the third deobfuscate is replaced by `odttf::deobfuscate`. |
| S15 | **The uo-jsnum dependency does not match uo-jsnum's plan.** `core/docs/modules/uo-jsnum.md` (stage 1, written in parallel) plans no lowercase helper (`lower_cp` is absent) and `parse_int(s: &str, …)` (S4 needs code units), and lists uo-fonts as needing log2, `**`, round, min/max, parseInt 10/36 — which this plan removed. `core/clippy.toml` has no `disallowed-methods` yet (the plan says it applies). Full-string `toLowerCase` is used in 5 places (liang.ts:54, embedding.ts:101/108/110, index.ts:34, types.ts:89, ooxml.ts:126/128/195/292) with the same V8-vs-Rust Unicode risk as `lowerCp`, unchecked. | no | Do not gate uo-fonts on uo-jsnum: a `pub(crate) mod js` (JS space set identical to uo-opc's, `trim`, `lower_cp`, `to_lowercase`, 2-unit hex `parseInt`) moved to uo-jsnum when it passes gate 2; add `js_to_lowercase` checked against V8 over every code point and the Final_Sigma contexts; fix the uo-fonts line of uo-jsnum.md. G8 revised. |
| S16 | **Memory budgets do not hold on real fonts.** "Fonts without GPOS ≤ 64 KB" fails for CJK: Meiryo 14,335 cmap ranges, Yu Gothic 14,077 (measured) × ≈ 32 B ≈ 460 KB. Gabriola's 742k pairs in a hashbrown map ≈ 2^20 buckets × 9 B ≈ 9.4 MB per face; Calibri × 4 ≈ 1.1 M pairs. In the browser the bytes live twice (JS `Uint8Array` for `FontFace` + the wasm copy), and `.ttc` → `Cow` → `Arc<[u8]>` copies a 20 MB face again. Without the TS `gposCache` (sfnt.ts:54) or the C# `ConditionalWeakTable` (TtfProgram.cs:72), the native/.NET path parses GPOS once per `Face::parse` (the editor did 71 parses for 19 fonts, sfnt.ts:49–51); the plan moves dedupe to the bindings only. | no | cmap as record offsets sorted by start (decode at query, ≈ 4–8 B per range); keep the hash `PairMap` but record opt-in M3 budgets for Gabriola, Calibri × 4 and Meiryo (a frozen sorted `Box<[u32]>` + `Box<[i16]>`, 6 B/pair, is a stage-5 candidate); `standalone_face` output convertible to `Arc<[u8]>` without a second copy; a capped `FaceCache` in the Rust layout provider plan. G14. |
| S17 | **`ooxml` splits ownership of OOXML parts.** `word/fontTable.xml` would be read by uo-docx (`readEmbeddedFonts`, XML parser, docx-core fonts.ts:42–73) and by uo-fonts (`readDocxEmbedding`, regex, ooxml.ts:189–204), and settings/fontTable rewritten by regex in uo-fonts while uo-docx owns the lossless writer. Today it is a post-pass on the saved bytes (universal-write.ts:2298 `DocxPackage.load(this.saveDocx())`, universal-slides.ts:452–456). | no | Approve G2 only as a documented post-pass over `uo_opc::Package`, or defer module 18 to the uo-docx/uo-pptx plans (the crate then has no Opus module and no uo-opc dependency). |
| S18 | **`FontProgram::legacy_kern` has no parity source.** The TS `TtfProgram` has no kerning (converter pptx/sfnt.ts); the C# reads `kern` eagerly, without the version check, last pair wins, cleared on error (TtfProgram.cs:170–201), used only by DocxPdf.cs:204. Nothing in the parity plan captures it. | no | Lazy (`OnceLock`), the C# rules exactly (no version check), bounded by `max_kern_pairs` and the work budget; captured by a small C# test emitting the same stream format over the corpus and the quirk fonts. G16. |
| S19 | **Office assumptions in the shared API names.** `kern_units` is the Word/DirectWrite emulation (union over all scripts, first value wins, lookup flags ignored — gpos.ts:58–69), and `EmbeddingApp { Write, Slides }` hard-codes app policies (embedding.ts:31–40) in the shared crate. | no | Document `kern_units` as the Office kerning emulation; `plan_font_embedding` takes `common_fonts: &[&str]` and `all_variants: bool` (the per-app tables stay as consts), so Sheet/others reuse it without an app enum. G13. |

### Sonnet readiness (modules 1–17)

The plan is unusually complete: types, catch points, Map/order/UTF-16 rules and facets are fixed.
Today a Sonnet translator would still have to decide: the outline work bound (S1), the NaN
offset state and outcome in the subset (S2), the subset output cap and its outcome (S3), odttf
code units (S4), which loops count against the budget and what each does over it (S7), the cmap
cap (S8), whether unused reads stay (S9), the panose clamp (S10), the nine items of S11, the
cache binding (S12), the embedding API shape (S13) and the `legacy_kern` source (S18). Once
these are written into the plan, modules 1–17 are a translation; the riskiest for Sonnet are
`gpos.rs` and `subset.rs` — give each a short pseudo-code of the budget/NaN rules and ask the
parity verifier to fuzz them differentially against the TS. `ooxml` stays Opus, or is deferred
(S17). The JS math is not a risk here: the only floating-point operations are `/ 255`,
`/ 16384`, `/ 65536` and the outline transform in source order.

## Stage 3 — port

### Phase 1 — foundation (Sonnet, `crates/uo-fonts`)

**What exists.** The crate skeleton (`Cargo.toml` with `rustc-hash = "=2.1.3"`, feature `hyphen-data`, dev-deps `uo-alloc` with `global` and `uo-testkit`; `lib.rs` with `#![forbid(unsafe_code)]`, every module of the plan declared and the API of the plan re-exported at the root), `error.rs` and `limits.rs` complete for all modules, and modules 0, 1 and 3 ported. Modules 4–17 are files with the plan's types and signatures and placeholder bodies, each starting with a one-line comment that names its phase-2 frontier.

| Part | State |
|---|---|
| `error.rs` | **complete**: `FontError { OutOfBounds, MissingTable(Tag), NoCmapSubtable, CmapFormat(u16), TableOutsideFile, Limit(LimitKind), NotPorted }`, `LimitKind { KernPairs, TableWork, OutlineWork, OutlinePoints, CmapRanges, OutputBytes }`, `From<OutOfBounds>`, `Display`, `Result`. `NotPorted` is the stage-3 placeholder; it goes away when frontiers B, C and D are done. |
| `limits.rs` | **complete**: `FontLimits` (the seven fields of the plan), `NATIVE`, `WASM32` (pair cap 2^22, cmap ranges 2^22, output 64 MiB, G5), `DEFAULT` by `target_pointer_width = "32"`, `FontLimits::work()`; and `WorkBudget` (`tick`, `tick_n`, `used`), the counter of R-BUDGET that every module uses (one per public call; over the limit it returns `Limit(TableWork)` and stays stopped, the caller maps it to the outcome of its own TS catch). |
| `js.rs` (module 0) | **ported**: `is_js_space(u32)`, `js_trim`, `split_js_space`, `lower_cp`, `js_to_lowercase`, `parse_int_hex(&[u16]) -> f64` (exact rounding for any length: the 30 leading digits plus a sticky bit, then scaling by 16), `to_uint8(f64)` (no `as` on a saturated value: magnitudes of 2^63 or more give 0). Private (`pub(crate)`), `#[allow(dead_code)]` on the module until the ports of frontier D use it. |
| `read.rs` (module 1) | **ported**: `View` (`u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `tag`, `slice`, clamped `subarray`, `signed_offset(i64)` for the hmtx index of -1), `OutOfBounds`, `Tag` (consts for the tables the plan names, `from_str`, `Debug` as a string). Offsets are `u64`, every add is checked, every `usize` is `try_from`. |
| `sfnt.rs` (module 3) | **ported**: `TableRecord`, `TableDir` (`parse`, `parse_at(base)`, `last` = `Map.set`, `first`, `map_order` = the order of the TS `Map` for the `dir` facet, S11d), `table_of(view, base, tag)` (the shared `tableOf`: first match, later records never read), `is_font_file`. The directory grows record by record, never from the declared `numTables`. |
| `uo-testkit` `generate::sfnt` | **new**: `build_sfnt(tables, opts)`, `sfnt(&[(tag, data)])`, `build_ttc(faces, version)`, `SfntTable` (offset, length and checksum overrides, repeated tags), `SfntOptions` (`version`, declared `numTables`, search fields), `BeWriter` (big-endian table content, with patching) and `checksum`. No proprietary font bytes anywhere. |

**Stub list per frontier** (phase-2 porters replace the bodies; they may only add to `error.rs` and `limits.rs`):

| Frontier | Files | Placeholder today |
|---|---|---|
| B | `cmap.rs` | `Cmap`/`CmapKind` types as in the plan; `glyph_ids_for` returns `Err(NotPorted)` |
| B | `face.rs` | `FontMetrics`, `Face` (`Arc<FaceData>` with every field of the plan's data model, `#[allow(dead_code)]` until `parse` builds it); `Face::parse` returns `Err(NotPorted)`; the queries return 0, `None` or `true` (`kern_in_latin`); `bytes`, `metrics`, `num_glyphs`, `table` read the (never built) data |
| B | `vdmx.rs`, `colr.rs`, `outline.rs` | the types (`VdmxEntry`, `VdmxTable`, `ColrInfo`, `ColorLayer`, `OutlineCmd`, `OutlineCache`), no functions |
| B | `program.rs` | `FontProgram::parse` returns `Err(NotPorted)`; accessors and `legacy_kern` return 0 |
| B | `info.rs` | `is_embeddable` returns `true` and `has_true_type_outlines` `false` (the TS catch values), `face_info` returns `Err(NotPorted)`; `FaceInfo`, `FaceNames` as in the plan |
| B | `ttc.rs` | `standalone_face` returns `Err(NotPorted)` |
| C | `gpos.rs` | `PairMap` (`new`, `len`), `MapFull`, `Class0`, `ClassDef`, `Kerning`; `PairMap::insert_if_absent` returns `Err(MapFull)`; `Face::kern_units` and `kern_in_latin` placeholders |
| C | `gsub.rs` | `GsubLigatures`, `Ligature`, `LigLookup`, `LigGroup`; `Face::ligate` returns `None` |
| D | `subset.rs` | `subset_true_type` returns `Cow::Borrowed(bytes)` (the TS catch outcome) |
| D | `eot.rs` | `sfnt_to_eot` returns `Err(NotPorted)` |
| D | `odttf.rs` | the four functions return the data unchanged (copy / `Cow::Borrowed`) |
| D | `variant.rs` | `variant_of_style` returns `None` (see decision 3 for the rest of the file) |
| D | `catalog.rs` | `bundled`, `metric_substitute` return `None`, `bundled_file_name` returns `""`, `METRIC_SUBSTITUTES` and `PROBE_FAMILIES` are empty; no table is generated yet |
| D | `hyphen/{mod,liang,registry,cache}.rs` | `Hyphenator::new` keeps `id`, `left`, `right`; `points`/`hyphenate` return empty; `HyphenSession`, `HyphenationRegistry` (`register` does not store, `for_lang` returns `None`), `hyphenation_pattern_id` returns `None`, `pattern_ids_to_load` is empty, `embedded_patterns` (feature) returns `None`; no `data/hyphen` files yet |
| D | `embed.rs` | `embedding_requests` returns an empty list, `plan_font_embedding` returns `Err(NotPorted)` |

**Tests (all from `core/`, release).** Unit: `js` 10 (the V8 replay below), `read` 5, `sfnt` 6, `limits` 2, `error` 1. Integration: `tests/sfnt_dir.rs` (6: generated fonts, `Map` rules, lying counts, collections, truncation at every byte), `tests/foundation_memory.rs` (4: M1 on every public operation of `read`, `sfnt`, `limits`, `error`, success and error paths, plus a measured check that a declared `numTables` of 65,535 on a 100-byte file allocates under 1 KB), `tests/api_surface.rs` (7, 8 with the feature: the compile-time contract of the whole API, `Send + Sync` of the handles, M1 on every placeholder). `uo-testkit`: 7 unit tests of `generate::sfnt`.

**`js` against V8** (`tests/js_support/gen_js.mjs` writes `tests/js_support/js_expected.txt`, deterministic, header `@node`/`@unicode`; regenerate with `node gen_js.mjs`). Replayed over: the `\s` set and `trim` for all 1,114,112 code points; `lower_cp` (the liang.ts rule, copied into the generator) and `toLowerCase` for every non-surrogate code point; 3,079 Final_Sigma strings (every string of up to 4 of 8 symbols that contains a capital sigma, plus seeded longer ones over 11 symbols and their reverses); `parseInt(s, 16)` for 149 first units × all 128 ASCII second units (the surrogates, the whitespace set, the signs included), 1,500 seeded strings and 67 hand-picked ones (prefixes, signs, 30–300 digit strings around the rounding and overflow edges, lone surrogates, astral); ToUint8 for 455 values (2^53, 2^63 and 2^64 edges, ±Infinity, NaN, ±0); `split(/\s+/)` for 736 strings. **Result: no difference** (Node 24.15.0 / ICU Unicode 17.0 against Rust 1.97.1 std), so no Unicode-version difference has to be listed.

**Decisions the plan did not settle** (everything else is the plan as written):

1. *V8 instead of uo-opc for the whitespace table.* R-JS asks for a test that `is_js_space` agrees with `uo_opc::xml::chars` over every code point. `chars` is private to uo-opc (and uo-opc is not touched in this phase), so the test compares `is_js_space` and `js_trim` with V8 itself over every code point. Stronger than the plan's check, since V8 is the oracle both tables answer to.
2. *Data-model types of modules 4–8 exist already.* The frontiers B and C run in parallel and `Face` (B) holds the kerning and ligature types (C), so the plan's data model is declared in phase 1 (`Cmap`, `CmapKind`, `ColrInfo`, `VdmxTable`, `PairMap`, `Kerning`, `Class0`, `ClassDef`, `GsubLigatures`, `Ligature`, `LigLookup`, `OutlineCache`, `FaceData`), with the fields the plan names, public fields where other modules read them. Two shapes are mine: `VdmxTable { entries: FxHashMap<u16, VdmxEntry> }` (the plan gives none; it is the TS `Map` of the chosen ratio) and `PairMap { map, overflow, cap }` (the plan's pseudo-code calls `PairMap::new(limits.max_kern_pairs)`, so the cap is a field). Frontier B and C may reshape their own types.
3. *Trivial pure code translated in stub files* where there is no neutral value or the signature is data: `Variant`, `VARIANTS`, `variant_of`, `is_bold_variant`, `is_italic_variant` (variant.rs); the consts `DEFAULT_FAMILY`, `HYPHENATION_PATTERN_IDS`, `WORD_COMMON_FONTS`, `POWERPOINT_COMMON_FONTS`, `WORD_DEFAULT_EMBEDDING`, `POWERPOINT_DEFAULT_EMBEDDING`; `PairMap::new/len`. The tables of catalog.ts stay empty (G9: generated once in frontier D).
4. *`Tag::from_str` accepts four characters that are all U+0000..=U+00FF* (the TS tags are Latin-1 strings), a superset of the plan's "4 bytes" for ASCII.
5. *Placement*: `glyph_ids_for` is in `cmap.rs` (module 4 of the table lists the shared `glyphIdsFor` there), `OutlineCmd` in `outline.rs` (the plan lists it under the `face.rs` block of the API; re-exported at the root, so the path `uo_fonts::OutlineCmd` is the plan's), `VdmxEntry` in `vdmx.rs`, `ColorLayer` in `colr.rs`, `LigGroup` in `gsub.rs`.
6. *`View::signed_offset(i64)`* is the plan's "one signed read path" (a negative offset is `OutOfBounds`, as `DataView` throws); `View::subarray` is the clamped `Uint8Array.subarray` (faceInfo's panose, subset's `slice` copies); `TableDir::parse_at(base)` and `table_of(view, base, tag)` carry the `base` of `.ttc` faces.
7. *Root re-exports beyond the plan's list*: `WorkBudget`, `TableDir`, `TableRecord`, `table_of`, `View`, `OutOfBounds`, `FxHashMap`, `FxHashSet` (the embedding plan's signature uses `FxHashSet`, so callers get the same type).
8. *Two `#[allow(dead_code)]`*: on the private `js` module (its users are in frontier D) and on the stub-only structs whose fields the ports wire in (`FaceData`, `FontProgram`, `HyphenSession`, `HyphenationRegistry`). Remove them when the fields are read.

**Notes for the orchestrator.** (a) `cargo clippy -p uo-fonts -p uo-testkit … -D warnings` currently fails on another session's work in `uo-testkit/src/capture.rs` (six `collapsible_if` in the platform checks added today), not on this phase's code: with `-A clippy::collapsible_if` both crates are clean. (b) `cargo fmt --all --check` cannot run while the untracked `uo-jsnum/tests/pow.rs` uses `gen` as an identifier (reserved in edition 2024); `cargo fmt -p uo-fonts -- --check` and `rustfmt --check` on the two `generate` files are clean. A first `cargo fmt --all` of this session ran before that was known; it only reformats, and the modified files in `uo-opc` and `uo-testkit` in `git status` are that session's substantive edits, not formatting. (c) `cargo build -p uo-fonts --target wasm32-unknown-unknown` compiles.

### Phase 2 — Frontier C: `gpos.rs`, `gsub.rs` (Sonnet)

**Ported** (replacing the phase-1 stubs; no other file of the crate touched, `error.rs`/`limits.rs` unchanged).

| Piece | API |
|---|---|
| `gpos.rs` | `PairMap` (`new`, `len`, `is_empty`, `contains_key(k: u64)`, `get(g1, g2)`, `insert_if_absent(k: u64, v)`), `MapFull`, `Class0`, `ClassDef` (`map`, `order`, `entries()`, `contains`), `Kerning` with `Kerning::parse(view, gpos: Option<TableRecord>, kern: Option<TableRecord>, &FontLimits) -> Kerning` (the whole `SfntFont` constructor logic: atomic GPOS, then the legacy `kern` when the map is still empty; infallible), `Kerning::of_font(bytes, &TableDir, &FontLimits)`, `Kerning::parse_gpos(view, off, limits) -> Result<Kerning, FontError>`, `Kerning::apply_legacy_kern(&mut self, view, off, limits)` (partial on failure), `Kerning::units(g1, g2) -> i32`, and the private `kern_in_latin(view, gpos)`. |
| `gsub.rs` | `GsubLigatures::parse(view, off, limits) -> Result<_, FontError>`, `GsubLigatures::of_font(bytes, &TableDir, limits) -> Option<_>`, `gsub::load(view, Option<TableRecord>, limits) -> Option<_>` (any failure is `None`), `GsubLigatures::ligate(&self, &[u32], &[Tag]) -> Option<Vec<LigGroup>>` (the `SfntFont.ligate` predicate) and `apply` (`applyLigatures`), `lookups_of(tag)`. |

**What `Face` has to call** (`face.rs` is frontier B's file; its stubs for these are still in place): `kern_units` = `self.0.kerning.get_or_init(|| Kerning::of_font(&self.0.bytes, &self.0.dir, &self.0.limits)).units(g1, g2)`; `kern_in_latin` = the same `.kern_in_latin`; `ligate` = `self.0.gsub.get_or_init(|| GsubLigatures::of_font(&self.0.bytes, &self.0.dir, &self.0.limits)).as_ref()?.ligate(gids, features)`. Until then the tests call the modules directly (generated fonts, `TableDir`).

**Tests** (all from `core/`, release; debug passes too). `tests/gpos_kern.rs` (20): PairPos 1 and 2 with the values pinned by hand, value-record shapes, feature union and lookup order (first value wins), coverage formats, class 0 (first entry wins, a listed g2 of class 0, referenced class definitions only, last value and first position in class definitions), extension lookups, key collisions and keys past 2^32, glyphs above 0xFFFF, a matrix that is never read, atomic GPOS and the legacy fallback, the Latin-script test in 13 script layouts, the legacy `kern` rules (8 fonts), the pair cap, the work budget pinned at its counted points (GPOS 56 units, legacy 8). `tests/gsub_ligate.rs` (10): font-order ligatures, the `None` conditions of `ligate`, singles (delta wrap, list shorter than the coverage, last subtable), lookup order and singles skipping merged groups, a lookup with both kinds, coverage format 3 read as format 2, script and feature lists (repeated tag, required feature, DFLT, no default language), the budget pinned (21 units), the stored-items cap. `tests/gpos_gsub_memory.rs` (6): **M1** `assert_no_leak` on `Kerning::parse`/`of_font`/`units`/clone and `GsubLigatures::of_font`/`ligate`/`apply`/clone over every generated font, and on every error path (pair cap, work budget at 11 values on 7 fonts, unreadable GPOS and GSUB, partial `kern`, `PairMap` operations); the hostile cases measured (below); a no-panic and no-leak loop over 3,400 mutated fonts (the tail sweeps plus 1,500 changes anywhere, directory included). `tests/gpos_gsub_parity.rs` (2, one `#[ignore]`d): the differential check.

**Differential against the TS.** `core/crates/uo-fonts/tests/gpos_support/`: `mod.rs` builds **62 fonts** byte by byte (no proprietary bytes; `examples/gpos_gsub_fonts.rs` writes them to a directory), `capture_gpos_gsub.mjs` runs the frozen `SfntFont` of `main` (`--ts-root` / `UO_TS_ROOT`, never a path in the repo; `@ts-commit d0f45bc5`, Node 24.15.0) over them and writes digests of canonical `uo-testkit` streams to `tests/parity/gpos_gsub.expected`; the test replays them over the Rust readers. Facets per font: the font bytes (stale-file check), `kern` (Latin flag, map size, every pair by key, keys past 2^32 as keys only, class 0 with its second-glyph class definition in insertion order), `units` (every pair of a grid of the font's glyphs plus 0xFFFF, 0x10000 and 0x20000, asked through `kernGlyphsTwips` at 30 pt and 600 units per em, where twips equal units), `gsub` (features in insertion order, lookups, singles), `ligate` (every ligature of every lookup as a sequence plus 150 seeded sequences, against 3 standard feature sets, each feature tag, the empty set and an unknown one). Sweeps: per base font (kern + GSUB + GPOS at the end of the file) **every truncation** of the tail and **400 seeded mutants** (1 to 3 bytes of the tail), the PRNG call sequence repeated in the script. **Result: all 314 facets equal** over the 62 fonts (class 0, extension lookups, format 1 and 2, every script layout, first value wins across lookups, the legacy fallback and its partial failures, the key collisions of `start + i` past 65,535 and the keys past 2^32, singles versus ligatures) and 1,658 sweep variants (2.9 million items of mutant kerning and GSUB state). The **V8 `Map` limit is checked at its real size** (`#[ignore]`, 300 MB, 2 s): a class matrix that expands to 4096 x 4096 = 2^24 pairs reads, 4097 x 4097 is the TS `RangeError` (GPOS unreadable), both equal to the TS run with `--max-old-space-size=8192`. Not committed, run once as a check: the same kern, GSUB and shallow `ligate` facets over the 26 open fonts of the spike corpus and **457 fonts of `C:\Windows\Fonts`** (the opt-in local corpus, read only): identical to the TS on every one (Gabriola 742,163 pairs, the Calibri family, Segoe UI Emoji, the CJK fonts); none has class-0 kerning, as the skeptic measured (S5), so class 0 is covered by the generated fonts only. The full capture over the corpus is stage 4.

**Memory, hostile input** (measured with `uo-alloc`, release): a 63 KB font with 1,500 subtables whose class-0 entries each keep a 65,536-entry class definition, and a 120 KB font whose 30,000 ligature sets point at one set of 30,000 records. Before the cap of decision 5: 872 MB peak and 6.5 s for the GSUB, an estimated 1.3 GB for the GPOS (2^26 class-definition entries at about 20 B). After: GSUB 218 MB with the native limits and 55 MB with the wasm32 ones, GPOS 371 MB and 94 MB. A 65,535 x 65,535 class matrix: 8 KB allocated without the XAdvance bit, under 2 MB with it; a coverage of 65,535 full ranges: 8 KB; a ligature of 59,999 components shared by 60,000 records: under 8 MB with a 2^20 budget. Real fonts: the largest stored GSUB of the Windows corpus is 9,103 entries (Gabriola), 460 times under the wasm32 cap; no real font keeps a class definition.

**Decisions the plan did not settle** (everything else is the plan as written):

1. *The `Face`-side contract.* The plan says what `Face::kern_units` and `ligate` do, not which functions of `gpos.rs` and `gsub.rs` they call. I made `Kerning::parse`/`of_font` (the constructor's GPOS-then-legacy logic, infallible), `Kerning::units` (the stored pair, else class 0 when g2 is not a key of the entry's class definition; a stored zero hides class 0; a glyph above 0xFFFF gives 0) and `GsubLigatures::of_font`/`ligate`. The wiring in `face.rs` is not done (not my file).
2. *Reshaped types.* `ClassDef` gained `order: Vec<u32>` (the first-insertion order of the TS `Map`, which the pair loop and the capture facet need; `map` keeps the last value), `entries()` and `contains`. `PairMap` keeps the phase-1 shape (`cap` field) and gained `contains_key` and `get(g1, g2)`; an overflow key (`g1 == 0xFFFF`, second glyph above 0xFFFF) is `2^32 + low 16 bits`, kept as those 16 bits. `Kerning::empty(limits)`, `parse_gpos` and `apply_legacy_kern` are additions.
3. *Coverage kept as runs*, not the plan's `Vec<u16>`: a coverage of 65,535 full ranges is 4 billion entries in the TS and, within the budget, 128 MiB of `u16` in a `Vec`; runs (`(start, end)`, 4 B each, at most 65,535) are iterated in table order and cost no memory (shared with `gsub.rs`, `pub(crate)`). Budget units are unchanged: one per entry, counted in bulk per range.
4. *Budget points as implemented* (the plan's table names them, the units follow). GPOS: one per kern-feature lookup index, per coverage entry, per class-definition entry (a range counts its glyphs, in bulk), per class-matrix cell, per `(g1, g2)` of the pair loop (class-1 glyphs x second-class entries), per format-1 pair record; without `vf1 & 4` the matrix and the pair loop are counted in bulk (`c1n * c2n`, `class1 * cd2`) and nothing is stored or visited (the same total, so the same outcome). Legacy `kern`: one per subtable, one per pair record. GSUB: one per feature index appended, coverage entry, single substitution and ligature record, **and one per component glyph of a ligature** (not in the plan: one record can be pointed at by 65,535 ligature entries, 8.6 GB of components in the TS). GPOS and the legacy `kern` each get a fresh budget (the plan's table lists them as separate rows with different outcomes; a shared one would let an exhausted GPOS starve the fallback). Pinned by tests: `g_pp2_basic` needs exactly 56 units, `k_basic` 8, `s_basic` 21.
5. *Memory caps the budget does not give.* The budget bounds time; with 2^26 units a hostile table keeps hundreds of MB (measured above). Two caps on what is **stored**, both against `FontLimits::max_kern_pairs` (no new knob, `limits.rs` untouched): the entries of the second-glyph class definitions kept for class-0 entries (GPOS; past it the GPOS is unreadable, `Limit(KernPairs)`), and the ligature records, component glyphs and single substitutions of a GSUB (past it the GSUB is `None`). No real font comes within 400 times of either. The doc of `max_kern_pairs` in `limits.rs` should say it also bounds these (left to the orchestrator: not my file). Whether 2^24 on native (a 300 MB pair map in the legitimate worst case) is the right size for them is a gate-2 question.
6. *The legacy `kern` reads a value only for an absent key* (`if (!has(key)) set(key, getInt16(r + 4))`): a repeated pair whose value bytes are cut by the end of the file is not an error, a new one is (partial result). The plan's pseudo-code says `insert_if_absent` for both tables; GPOS reads the advance first (so always), `kern` does not. Zero values are stored by `kern` (they hide class 0); GPOS never stores zero.
7. *The Latin test inside the atomic read.* `kern_in_latin` runs after `parse_kern` even when the GPOS has no `kern` feature (the plan's pseudo-code returns early with a comment); a failure of it (for example a feature index whose tag lies past the end of the file) makes the **whole** GPOS unreadable, pairs included, as the TS `try` does (tested: `g_script_feature_index_oob`).
8. *Small shapes.* `LigGroup.len` adds with `saturating_add` (no wrap for more than 2^32 input groups); `ligate` and `apply` take `&[Tag]` (the TS passes any string; four Latin-1 characters in practice); the lookups of a tag are found by linear search over the (few) distinct tags.
9. *Differential design.* The fonts come from Rust and the Node script reads files (no second font builder in JS), so one generator feeds both sides; the sweeps (truncations, mutants) are generated on both sides from the shared `Rng` and the same call sequence, only the base fonts are written to disk. The query sets are derived from the data each side parsed (a missed pair already shows in the `kern` digest); the units grid is capped at 400 glyphs (named fonts only, the sweeps skip it).
10. *Not done.* The libFuzzer targets `gpos_kern` and `gsub_ligate` of the plan (`core/fuzz` is not my file; the mutation loops are the in-crate stand-in), a wasm test run (only the build), Miri (no `unsafe`: `#![forbid(unsafe_code)]`), M2 and M3 numbers (stage 4). The C# `Gpos.JsMapMaxSize` finding stays as the plan states it.

### Phase 2 — Frontier D: `subset.rs`, `eot.rs`, `odttf.rs`, `variant.rs`, `catalog.rs`, `hyphen/*`, `embed.rs` (Sonnet)

**Ported** (replacing the phase-1 stubs; `error.rs`, `limits.rs` and `lib.rs` untouched; no frontier-D function returns `NotPorted` any more).

| Module | What it is now |
|---|---|
| `subset.rs` | `subset_true_type` as R-SUBSET: `Off::{At, Nan}`, the component loop that gives the input when it would never leave the `Nan` state (S2), output size computed before any allocation and checked against `max_output_bytes` (S3), the int32 rule of `(len + 1) & ~1`, one work budget (pops and component records), a 65,536-bit set for the kept ids. |
| `eot.rs` | `sfnt_to_eot` = `face_info` + `eot_with_info` (the layout, `pub(crate)`). Names as UTF-16LE code units, panose bytes as the clamped hex, sizes modulo 2^32 / 2^16 as `setUint32/16` store them. |
| `odttf.rs` | the four functions over UTF-16 code units (`js::parse_int_hex` + `to_uint8`), R-ODTTF. |
| `variant.rs` | `variant_of_style` (lower case, JS whitespace runs to one space, JS trim); `Variant::name()` (the TS names). The rest of the file was translated in phase 1 (verified against the TS). |
| `catalog.rs` | lookups, `bundled_file_name`; the tables are `src/catalog_tables.rs`, generated once by `tools/gen_catalog.mjs` (G9) and included. |
| `hyphen/` | `Hyphenator` (trie built with hash maps, frozen into CSR arrays, binary search per step), `HyphenSession` (cache bound to its hyphenator, cleared past 50,000 words), `HyphenationRegistry` (`register` replaces in place), `hyphenation_pattern_id`, `pattern_ids_to_load`, `parse_patterns` (the text format), `embedded_patterns` (feature `hyphen-data`), and `data/hyphen/<id>.txt` generated once by `tools/gen_hyphen_data.mjs` (G7). |
| `embed.rs` | `embedding_requests` and `plan_font_embedding` over one shared walk (grouping, skip rules, wanted variants), R-EMBED. |

**Tests** (all from `core/`, release; debug passes too, so no arithmetic overflow either). In the crate: 45 unit tests (`odttf` 9, `variant` 3, `catalog` 5 including the re-parse of `catalog.ts` while it exists, `eot` 6, `hyphen` 22). Integration: `tests/subset.rs` (19: what is kept and written, both loca formats, composites in every transform form, S2 and S3, the cap checked before allocating (measured), the work budget, 4,000 mutated fonts and every truncation, M1 on every path), `tests/subset_real.rs` (5: the 16 open fonts of the repository through `Face`, ids/advances/outlines kept, unused glyphs emptied, GPOS/GSUB/GDEF copied whole and the kerning equal, EOT fields, M1, M3), `tests/embed.rs` (11: both phases, skip order, licence, subset decision, `subsetted` when the subsetter gave the input back, errors reject the plan, M1), `tests/eot_odttf.rs` (4: 30,000 random keys against an independent model of `parseInt`, involution, 3,000 mutated fonts, M1), `tests/hyphen_data.rs` (7: the seven files, known words, registry and session, M1/M3, M5, hostile patterns, the embedded files with the feature), `tests/frontier_d_soak.rs` (2: M2). Generated fonts come from `tests/frontier_d_support` (a TrueType builder over `uo_testkit::generate::sfnt`).

**Differential against the TS** (Node 24.15.0, frozen `main` d0f45bc5; the capture scripts take `UO_TS_ROOT`, the Rust side generates the fonts, the captures hold digests only):

- `subset_true_type` and `sfnt_to_eot`: `tests/subset_parity.rs` replays `tests/parity/subset.cap` over **588 cases** (generated fonts: every composite form, both loca formats, missing/duplicate/lying tables, S2 and S3 cases, truncations every 37 bytes, 360 seeded mutations of directory, head, loca and glyf; plus the 16 open fonts x 3 glyph sets x 2 modes). **Result: 2,350 facets equal, 0 mismatches** (subset digest, EOT digest or throw for both flags, input digest); **2 accepted**: the two S2 cases the TS never returns from (`hang` in the capture, watched by a worker with a 3 s watchdog), where Rust returns the input (G11). 519 cases produce a subset, 69 give the input back. The 59 EOT throws agree with the TS too, which also exercises frontier B's `face_info`.
- Small facets: `tests/frontier_d_parity.rs` replays `tests/parity/frontier_d.cap` over the shared corpus `tests/parity/frontier_d_inputs.txt` (generated by `gen_frontier_d_inputs.mjs`, read by both sides): **295 odttf keys** (valid, short, signs, spaces, `0x`, non-ASCII, astral, lone surrogates) for `obfuscate` and `deobfuscate`, **119 style names**, **266 language tags** (case, `_`, prototype names), **11,912 hyphenation words in the 7 languages** (every exception word, seeded words from each language's pattern letters, real words, casing variants, `İ`, `Σ`, `ß`, ligatures, astral, soft hyphen; `points` and `hyphenate` for minimums 1..=3, digests of chunks of 250 words), and the catalog lookups. **Result: 1,757 facets equal, 0 mismatches, 0 accepted.**
- Not differential: `embed` (the stub resolver of the plan's `embed` facet is stage 4; covered by unit tests here).

**Memory** (`uo-alloc`, release): M1 on every public operation and error path (subset including the S2 exit, the cap and the budget; EOT; odttf; the plan ok and rejected; hyphenator, session, registry; catalog, variants). M2: 600 rounds of subset (both modes) + EOT + odttf + embedding requests and plan + hyphenation with a fresh session (400 words), sampled every 50, **flat at 0 bytes**; a long-lived hyphenator with a session over 240,000 words stays under its cap. M3 first references: subset of Carlito Bold (690,516 B, 2,782 glyphs) to 33 characters: output 37,956 B (152,936 B with the layout tables), allocated 94,790 B (210,538 B), peak 75,264 B (190,628 B); EOT of the same font: 690,940 B output, 692,284 B allocated; the de-1996 hyphenator (36,709 patterns) **keeps 2,188,164 B** (budget 4 MB), 11.8 MB allocated and 5.9 MB peak while building. M5: the session cache is capped (tests fill it past 50,000 and 120,000 words).

**Checks** (from `core/`): `cargo fmt -p uo-fonts -- --check` clean for these files; `cargo clippy -p uo-fonts --all-features --lib --tests -- -D warnings` clean for every frontier-D target (the findings left in the package are in `tests/b_support`, `tests/gpos_support` and `tests/face_parse.rs`, not mine); `cargo test --release -p uo-fonts` green with and without `hyphen-data`; `cargo build -p uo-fonts --target wasm32-unknown-unknown` builds; `core/tests/float-casts.sh` and `core/tests/grep-bans.sh` exit 0.

**Decisions the plan did not settle** (everything else is the plan as written):

1. *EOT split for testability.* `sfnt_to_eot` calls `face_info` and then `eot_with_info(font, &FaceInfo, subsetted)` (`pub(crate)`), so the layout is tested with hand-made values independent of the font parser. The panose bytes use the general `parseInt` path (`parse_int_hex` over the clamped hex units, then `to_uint8`), not the plan's "pair value or 0" shortcut; the results are the same for every string `face_info` produces.
2. *EOT sizes wrap, they do not fail.* `EOTSize` and `FontDataSize` (modulo 2^32) and each name size (modulo 2^16, for a name over 65,535 bytes) are stored as `DataView.setUint32/16` stores them. The output is bounded by the input and by the four names, so no cap is needed.
3. *The int32 rule of the subset is the TS one, not the pseudo-code's shortcut.* The pseudo-code says `len + 1 >= 2^31` gives the input. The TS computes `ToInt32(len + 1) & ~1`: negative from `2^31 - 1` up to `2^32 - 2`, and **zero for `len = 2^32 - 1`** (then `part.set` throws when the glyph has bytes inside the file, so the input comes back; a glyph wholly past the end of the file would give an empty part). Implemented as the TS computes it (`as u32 as i32`, a negative gives the input, a `subarray` longer than the padded length gives the input); every observable outcome is the pseudo-code's.
4. *Cap accounting in the subset.* `total = 12 + 16 n + sum of pad4(len)` with the lengths the TS allocates: `glyf` = the sum of the padded kept lengths, `loca` = `(numGlyphs + 1) * 4`, `head` = its **declared** length (so a lying `head.length` is caught by the cap before any allocation), every other kept table = its length **clamped** to the input (`slice`). A cap equal to the output size lets it through (tested).
5. *Kept-glyph set.* A fixed 65,536-bit bitmap (every id is a `u16`); ids above `u16::MAX` in `used_gids` are ignored (the TS `g < numGlyphs` filter, `numGlyphs <= 65,535`). The worklist order is the TS one (0, then the used ids in order of first appearance, LIFO), although every outcome is order independent.
6. *Differential design for the subset.* Rust is the only generator of the cases (a second font builder in JS would drift): an opt-in test writes them to a folder (`UO_FONTS_CASES_DIR`), the Node script runs the TS in worker threads with a watchdog (the S2 cases never return) and writes the digests, and the replay regenerates the same cases and checks an `in` digest per case, so a changed generator reads as "recapture", not as a mismatch. The case `int32-max-minus-one` (a 2 GiB allocation in V8) is not in the TS corpus; it is a Rust test of the cap. The corpus reads the repository's open fonts when they are present.
7. *odttf: the dash is gone before parsing.* `-` is removed with `{` and `}` before the 32-unit check and the slices, so a sign other than `+` never reaches `parseInt` (as in the TS); the plan's `-1` key example cannot occur there. `+`, spaces, `0x` and surrogates are covered.
8. *Catalog tables as an included file.* `lib.rs` is not mine, so `catalog.rs` has `include!("catalog_tables.rs")` and the generator writes that file already formatted (rustfmt does not follow `include!`). It carries `DEFAULT_FAMILY` too (the phase-1 constant, verified equal) and `pub const BUNDLED: &[(&str, BundledFamily)]` (a table the plan names but gives no constant for). Lookups are exact matches over own keys (the caller lower-cases, as the API doc says). The generator imports the **built** `shared/fonts/dist/catalog.js` (insertion order; integer-like keys asserted absent); the re-parse test reads `shared/fonts/src/catalog.ts` and skips with a message when it is gone.
9. *Hyphenation data format.* `data/hyphen/<id>.txt`: `#` lines (the licence banner of each TS module, copied from its `.ts` source because `tsc` drops it), `id`, `left`, `right`, `[patterns]`, `[exceptions]`, tokens separated by a space or a line break (wrapped at 120 columns); `THIRD-PARTY-NOTICES.md` is copied beside them. The generator reads the pattern data from the built dist modules, requires every token NFC **and** every exception NFC with its dashes removed (the TS key is `replace(/-/g, '').normalize('NFC')` while the points come from the NFC form with the dashes in place), and fails otherwise. `uo_fonts::hyphen::parse_patterns(text)` is public (not re-exported at the root: `lib.rs` is not mine), so the shell passes the files it fetches through it and `embedded_patterns` uses it.
10. *Hyphenator details.* No NFC at build or query (the contract). The values of the root node are never applied (the TS looks at `node.v` only after taking a step, so a pattern without letters is inert); pattern digits are ASCII `0..9` only; exception points saturate at `u32::MAX`; the exception key is built for a lookup only when the exception table is not empty (same result). `hyphenate` takes its minimums explicitly (no default arguments).
11. *Additive API.* `HyphenationRegistry::{contains, get, ids}` (`pattern_ids_to_load` needs `contains`), `HyphenSession::cached_words` (the M5 test), `Variant::name`, `catalog::BUNDLED`, `hyphen::parse_patterns`. Nothing the plan lists changed.
12. *`pattern_ids_to_load` own keys.* A language equal to an id is taken as is (`HYPHENATION_PATTERN_IDS`, exact match), every other goes through `hyphenation_pattern_id` (so `DE` gives `de-1996`, as in the TS); `constructor` and `toString` are not ids (the TS `LOADERS[l]` matched them and then threw; accepted by name, as the plan says for G9).
13. *`embed.rs` shape.* One private walk (`Step::{Skip, Family}`) feeds both phases, so the requests are exactly the calls the plan makes (tested by recording the calls). The code points of a family are kept in a hash set and **sorted** before `glyph_ids_for` (the result does not depend on the order; sorted for determinism). The policy's common fonts are lower-cased with `js_to_lowercase` per call; `already_embedded` must come lower-cased from the caller, as the plan says. `FaceToEmbed.bytes` shares the `Arc` of the source when nothing was cut (no subset requested, no outlines, or the subsetter returned the input).
14. *Shared files touched.* `tests/api_surface.rs`: four assertions of mine (EOT, the catalog table sizes, the plan's skipped list, a test name) replaced because they asserted the placeholders; nothing else in it. One `cargo fmt -p uo-fonts` of mine reformatted the whole package (whitespace only; it also touched files of the other frontiers).
15. *Capture metadata.* `subset.cap` and `frontier_d.cap` record `@ts-commit` (`d0f45bc5`) and the Node/V8 platform line, like the other captures.
16. *Not done.* The libFuzzer targets of the plan for subset, EOT, odttf, hyphenation and the embedding plan (`core/fuzz` is not my file; the mutation and random-key loops are the in-crate stand-ins), a TS differential of the embedding plan (stage 4: needs the stub resolver), the wasm test run (only the build), Miri (no `unsafe`), M2 in wasm, and the M3 numbers in the record's verification table (stage 4; the numbers above are first references).

### Phase 2 — Frontier B: `cmap.rs`, `face.rs`, `vdmx.rs`, `colr.rs`, `outline.rs`, `program.rs`, `info.rs`, `ttc.rs` (Sonnet)

**Ported** (replacing the phase-1 stubs; `lib.rs`, `error.rs` and `limits.rs` untouched; no frontier-B function returns `NotPorted` any more).

| Module | What it is now |
|---|---|
| `cmap.rs` | `Cmap::parse` (`pub(crate)`, with `CmapScoring::{Face, Program}`): the `parseCmap` reads in the TS order (platform, encoding and offset of **every** record, then the format, then every segment/group in table order), format 4 with the `ceil(segX2 / 2)` loop over arrays laid out by the exact `segX2`, format 12, indices stable-sorted by start (`order`, 4 B per range), `max_cmap_ranges`. `Cmap::lookup` (the exact binary search; format-12 id saturated to `u32::MAX`; format-4 array read past the end is 0), `Cmap::glyph_id` (the `U+F000 + code` symbol fallback), `Cmap::range_count`. `glyph_ids_for`: the embedding plan's own reader (score (3,10)=3, (3,1)=2, (0,*)=1, no (3,0); the offset read only on a better score; linear scans; first `cmap` record of the tag), sorted, distinct, no 0, `Err` when unreadable. |
| `face.rs` | `Face::parse` in the R-READS order (directory, `head`, `hhea`, `maxp`, `OS/2`, numGlyphs, numberOfHMetrics, `hmtx`, metrics in the object-literal order with the OS/2 length/version rules, `post`, `cmap`), `glyph_id`, `advance_width` (signed index, -1 reads the table before `hmtx`), `is_symbol_font`, `vdmx`, `has_color_glyphs`, `color_layers`, `glyph_outline` (per-face cache). **Also wired, as the orchestrator asked after frontier C finished:** `kern_units` = `kerning.get_or_init(Kerning::of_font(..)).units(g1, g2)`, `kern_in_latin` reads the same field, `ligate` goes through `GsubLigatures::of_font(..)`; both parse on first use and are kept in the face. |
| `vdmx.rs`, `colr.rs` | `vdmx::parse` (first ratio that serves a square device, last record of a size wins, the whole table `None` on any failure in the group read), `colr::info` / `colr::layers` (`colrInfo`, `colorLayers`: palette index 0xFFFF or `>= nColors` is no colour, alpha `a / 255`, BGRA to `[R, G, B]`). |
| `outline.rs` | `glyph_outline` = `glyphOutline` + `glyphContours` + `contoursToPath`: simple glyphs (flags with repeat, short vectors), composites (all three scale forms, byte or word offsets, `!(flags & 2)` zeroes them, depth 8, 64 components, transform in the TS order with no fused multiply-add), empty glyph `Some([])`, the closing `L` before `Z`. |
| `program.rs` | `FontProgram`: the converter's `TtfProgram` (directory read as tag + offset only, requires `head`, `hhea`, `hmtx`, `cmap`; own cmap scoring; italic angle `i32 / 65536`), and `legacy_kern` = the C# `ParseKern` (lazy, no version check, last pair wins, everything cleared on any failure). |
| `info.rs`, `ttc.rs` | `is_embeddable`, `has_true_type_outlines`, `face_info` (+ `name_string`), the crate-private `postscript_name`; `standalone_face` (name scan as UTF-16 code units, `TableOutsideFile`, output computed in `u64` and capped before allocating). |

**Tests** (all from `core/`, release). New files: `face_parse` (19: metrics by OS/2 length and version, error order of the constructor, cmap scoring, symbol fallback, format 4 deltas/arrays/odd `segX2`/overlap against a model of the TS loop, format 12 and saturation, range cap, advance edge cases), `face_tables` (8: VDMX and COLR/CPAL), `outline` (18: every contour/composite rule, depth 8/9, 64 components, visit and point limits, cache), `info_ttc` (25: licence bits, `face_info`, names, `glyph_ids_for` incl. its budget, `.ttc` extraction byte for byte against `build_sfnt`, name scan, output cap), `program` (16), `face_kern_ligate` (9: the Face wiring over the frontier-C builders), `frontier_b_memory` (12: M1 on every public operation and error/limit path, M5 for the outline cache and the cmap index, no allocation from declared counts, a 400-iteration soak with tolerance 0), `frontier_b_nopanic` (5: seeded mutations of the 20 generated fonts and of two repository fonts, every truncation of a rich font, mutated collections, random bytes; every call under both the default and a tight `FontLimits`), `real_fonts` (3, plus 3 `--ignored` tools). The shared generator is `tests/b_support/mod.rs` (`Font`, `format4/12`, `simple_glyph`, `composite`, `vdmx`, `colr`/`cpal`, `name_table`, `kern_table`, `mutate`, `quirk_fonts()` = 20 fonts). `tests/api_surface.rs`: the five placeholder assertions of frontier B now state the real results (zeros: `MissingTable(head)`, an empty `face_info`, `Ok([])`, the input back).

**Checks against the TS (hand check, not the stage-4 capture).** `tests/b_support/ts_digest.mjs` (frozen `main` dist + the converter's `TtfProgram` run as TS; `UO_TS_ROOT`) and the ignored test `real_fonts::digest_dir` print the same 20 lines per font: open outcome (error class), metrics, cmap over every code point 0..=0x10FFFF (+ 0x110000, 0xFFFFFFFF) as count + FNV-64 of (cp, gid), advances, every outline (op codes and f64 **bits**), VDMX for ppem 0..=300, COLR layers, symbol test, kerning over 300 x 300 glyph pairs (`kernGlyphsTwips` inverted to units) and `kernInLatin`, `ligate` over fixed texts and feature sets, the converter program (metrics, bbox, italic-angle bits, cmap, advances), `isEmbeddable`, `hasTrueTypeOutlines`, `faceInfo` (names as code units) and `glyphIdsFor`. Results: the **16 repository fonts: 320 lines identical**; the **20 generated quirk fonts** (all cmap formats and scorings, odd `segX2`, overlap, array reads past the end, symbol cmap, format-12 saturation, OS/2 of every length, hMetrics 0, VDMX ratios, COLR palettes, composites with every scale form, depth, missing components, names with lone surrogates and odd lengths): identical but for the one font that has an end point past the last point (below); **3 runs of seeded mutations** (every facet: 600 mutants of the quirk fonts, 8,046 lines; 2,400 mutants of the quirk fonts, 32,055 lines, before the kerning and ligature lines were added; and, with all 20 lines, 1,320 mutants = 22 bases x 60, of which 120 mutate Carlito Regular and Liberation Mono Regular, 19,601 lines): identical but for the same two accepted outcomes, which showed up as outline lines (every one a glyph whose end point passes the last point; in the real-font mutants 2 glyphs of one Carlito mutant, with 2.7 million NaN commands each in the TS, and 1 of a Liberation Mono mutant) and one `glyph_ids_for` line (format-12 saturation). Nothing else differed, including the error class of every open failure. The tool also writes the fonts and mutants (`dump_quirk_fonts`, `dump_mutants`, `UO_DIGEST_DIR`, `UO_MUTANTS`), so the verifiers can reuse it. Not covered by it: `standalone_face` against the TS (checked against hand-built expected bytes only), `legacy_kern` (C# only; no C# capture was run), and the 2^24 `Map` and 2^27 array limits.

**Decisions the plan did not settle** (everything else is the plan as written):

1. *`cmap.rs` shape.* `Cmap::parse` is `pub(crate)` with a `CmapScoring::{Face, Program}` argument (the converter's scoring ranks (3,0) with the unknown encodings, so it never sets `symbol`); `lookup`, `glyph_id` and `range_count` are public. `FontProgram` holds the same `Cmap` type as `Face`.
2. *Building the index.* The `order` vector is pushed record by record and sorted with `sort_by_key`, reading the start from the bytes (stable, no temporary key vector; a read that fails there, which cannot happen after the parse read it, gives 0). The `max_cmap_ranges` check comes **after** the four reads of a segment/group and before its push, so a read past the end beats the limit, and a cap of 0 rejects any cmap with a range. On native the cap cannot trip on format 4 (at most 32,768 segments); only a test cap does.
3. *Arithmetic.* Table offsets are `u64`; additions of a `u32` offset and small constants or counts of at most 2^16 are plain (they cannot overflow `u64`), every `usize` is `try_from`; the binary search and `(c + delta) & 0xFFFF` run in `i64`; outline coordinates accumulate in `i64` (exact up to 2^53) and become `f64` once.
4. *`advance_width_at`* (the signed hmtx index) is a `pub(crate)` function of `face.rs`, shared with `FontProgram`, so both readers keep one rule.
5. *Outline cache.* A glyph at or past `numGlyphs` is `None` and never enters the cache; `outline_cache_entries == 0` turns the cache off (the plan says nothing about 0); a full cache is cleared whole before the insertion; the outline is computed outside the lock, so two threads asking for the same new glyph may both compute it (one result is kept; no lock is held while reading font bytes).
6. *Outline limits as implemented.* `max_outline_work` counts **every** entry of `glyphContours` (including depth 9 and non-existent components) with the check `visits > max` after the increment, so a limit of 0 rejects everything. `max_outline_points` is checked against **two** totals of one call: the points decoded (sum of `nPts`, which bounds the time of 4,096 visits x 65,536 points) and the points emitted into contours (a decreasing end-point array re-emits points: `[3, 0, 3, 0, ...]` over a 4-point glyph emits three or four for every pair, up to 2^31 from a small glyph); either over the limit is `None`. `head + 50` is re-read at every visit as in the TS, `numGlyphs` is the stored one (the same bytes).
7. *`FontProgram` shape.* Reshaped to `FontProgram(Arc<ProgramData>)` (cheap clone with a `OnceLock` for the kern pairs). `has_legacy_kern()` is **new API** (the C# `HasKerning`; the parity facet needs it). The directory is read as tag and offset only, as `TtfProgram` does, so `TableDir` is not used there.
8. *`legacy_kern` outcomes.* Pairs are an `FxHashMap<u32, i16>`; the lookup key is the C# `((long)left << 16) | (uint)right` in `u64`: a `right` above 0xFFFF is OR-ed into the high half and can hit another pair (as in the C#), a key above 32 bits matches nothing. Over `max_kern_pairs` (an absent key at the cap), over the work budget (one unit per subtable and per pair record) and any read past the end all **clear everything** (the C# `catch`); the plan's R-BUDGET row for `kern` says "partial", which is the TS `SfntFont` reader in frontier C, not this one.
9. *Budgets the plan did not list.* `face_info` has no `limits` parameter in the plan, so its four name reads share one budget of `FontLimits::DEFAULT` (one unit per name record and per character copied; `Err(Limit(TableWork))`): without it 65,535 records pointing at one 64 KB string cost 2 x 10^9 reads (tested with 3,000 records). In the `.ttc` name scan the units are one per face, per directory record scanned in that face, per name record and per character (the plan counts face x name record; the directory scan and the characters are the other unbounded factors).
10. *S10 is unobservable.* The clamped `faceInfo` panose is implemented (`view.subarray`), but whenever the panose range is cut by the end of the file the read of `fsSelection` at `OS/2 + 62` (which comes later in the TS object literal) already throws, so `face_info` is an `Err` and a short panose never reaches the caller or the EOT writer. The clamp stays as the plan states it.
11. *`standalone_face` details.* The output is assembled sequentially (header, directory, data), the allocation is `try_reserve_exact` (a failure is `Limit(OutputBytes)`), and the total is a `u64` sum, so the int32 wrap of `(len + 3) & ~3` cannot happen (files above 2 GiB give `OutputBytes`, where the TS throws or goes negative). `postscript_name` is `pub(crate)` in `info.rs`. The name is compared as UTF-16 code units (`str::encode_utf16`).
12. *`glyph_ids_for` details.* A format-12 id above `u32::MAX` is saturated (the TS set holds the larger number); a format other than 4 or 12 is `Ok(empty)` after the format is read; `n == 0` and a missing table are `Ok(empty)`; a code point above 0xFFFF in format 4 costs no work units.
13. *Accepted differences the differential found* (both already named in the plan, now measured): an end point past the last point of a simple glyph gives `None` where the TS returns an outline with `undefined` coordinates (G4); a format-12 id above 2^32 is saturated in `glyph_id` and `glyph_ids_for` (in one mutant, 256 distinct TS ids collapse into one; every consumer treats any id above `numGlyphs` alike).
14. *`kern_units`, `kern_in_latin` and `ligate`* are wired as frontier C documented; I removed the `#[allow(dead_code)]` of `FaceData` (every field is read now).
15. *Tests.* `b_support` is a test-only module shared by the frontier-B test files; the TS digest tool lives in `tests/b_support/ts_digest.mjs` (it runs the frozen TS from `UO_TS_ROOT` and the converter source from `UO_CONVERTER_SRC`, never a path in the repo).

**Not done.** The libFuzzer targets `font_parse`, `glyph_outline`, `colr_vdmx`, `font_program`, `info_ttc` (`core/fuzz` is not my file; the mutation loops of `frontier_b_nopanic` are the in-crate stand-in); the stage-4 capture and its `.cap` replay (`fonts.cap`); the C# capture of `legacy_kern` (G16); a wasm test run (only `cargo build --target wasm32-unknown-unknown` and clippy for that target); Miri (no `unsafe`); the M2 soak with the oracle corpus, M3 numbers and the opt-in local fonts (Gabriola, Meiryo, VDMX/COLR fonts), all stage 4.

## Verification (gate 2 — pending)

### Stage 4 — parity (parity verifier, Sonnet, 2026-11-03)

Verifier: Sonnet, rust-port stage 4, worktree `rust-core` (`c869abfb` plus the stage-4 edits of the memory and security verifiers that landed meanwhile: F6–F10; the tests below were re-run after them). Everything below comes from tests that ran. **Result: 0 mismatches and 0 port bugs (0 parity rounds)**: 17,436 facets compared (17,366 equal, 70 accepted by name) over 536 committed fonts, the non-font inputs and the C# capture, plus 14,521 facets over 620 local proprietary fonts and 40,156 over 1,800 extra mutants; one finding for the maintainer (the work budget of `glyph_ids_for`, below) that is a design limit, not a port bug.

**What exists** (all new files; nothing in `core/crates/uo-fonts/src` was touched):

| File | What |
|---|---|
| `tests/parity_support/capture_fonts.mjs` (+ `capture_fonts_small.mjs`) | the capture: the frozen TS (`UO_TS_ROOT`, `@ts-commit d0f45bc5`, Node 24.15.0 / V8 13.6, `@platform`) over the corpus, digests of `uo-testkit` canonical streams only, written to `tests/parity/fonts.cap` (665 KB). The per-font work runs in a worker watched by the main thread (a facet silent for 120 s, or a worker that runs out of its 4 GiB heap, is recorded as `hang`; the subsetter's S2 loop is cut by a `vm` timeout per case and recorded in `subset.hang`). Modes: `--corpus <dir>` (the committed capture), `--dir a,b --out <cap>` (the opt-in local capture over proprietary fonts, never committed), `--only`. |
| `tests/fonts_parity_support/{facets,corpus,legacy,legacy_corpus}.rs` | the facets of one font (the Rust side of every stream, the same items in the same order), the corpus (open fonts, embedded fonts extracted in memory through uo-opc, the three frontiers' generators, mutants, collections, hostile fonts, the embedding pool) and the G16 corpus |
| `examples/fonts_corpus.rs`, `examples/legacy_kern_corpus.rs` | write the generated corpus (Rust is the only generator: no second font builder in JS) and a manifest for the capture scripts; the replay regenerates the same bytes and checks an `in` digest per font |
| `tests/fonts_parity.rs` | replay of the fonts (3 tests by group, threaded over the cores; seconds in release) |
| `tests/fonts_parity_small.rs` | replay of odttf, style, lang, variants, catalog, hyphenation, `loadHyphenation` and the embedding plan |
| `tests/legacy_kern_parity.rs`, `tests/parity_support/legacy_kern.cs`, `tests/parity/legacy_kern.cap` | G16 |
| `tests/fonts_parity_local.rs` | opt-in (`UO_LOCAL_FONTS=1`) replay of the local capture |
| `tests/parity/fonts_oracle_words.txt`, `fonts_embed_scenarios.txt` (+ `gen_oracle_words.mjs`, `gen_embed_scenarios.mjs`) | shared inputs, read by both sides (1,567 letter runs of the 2,742 oracle documents; 772 seeded embedding scenarios) |
| `uo-wasm`: feature `fonts-selftest`, `src/fonts_selftest.rs`, `tests/fonts_selftest.test.mjs` | the wasm replay (separate feature, file and test from the memory verifier's `fonts-memtest` and the uo-jsnum features) |

**Corpus** (536 fonts; none proprietary): 22 open fonts of the repository (16 in `shared/fonts/files`, Liberation Sans ×4, Amatic SC ×2; the same bytes sit in several folders) and the 6 WOFF2 files plus the Chivo `.fntdata` as negative inputs; the 7 fonts embedded in `site/assets/samples/demo.docx` (Ubuntu ×4, Ubuntu Mono, Tahoma) and `emwidth/ew-emb.docx` (Amatic SC), extracted in memory; 128 generated fonts (frontier B's 20 quirk fonts, frontier C's 62 kerning and ligature fonts, 43 of frontier D's subset fonts, 3 hostile fonts); 12 `.ttc` collections (1, 4 faces, version 2, hostile `numFonts`, a first face past the end, table directories pointing at the whole file, tables past the end, truncated, mixed Mac/Unicode/lone-surrogate names, not a collection); 360 seeded mutants (8 of each of 45 bases: the 22 open fonts, the 20 quirk fonts, 3 composite fonts; half the mutations inside the first bytes of a table, some in the directory). Not committed and counted separately: the local capture and the extended mutant campaign.

**Facets per font** (`items:digest` of canonical streams; floats by bits): `open` (ok or the error class) and `dir` (`Map` order), `metrics`, `numglyphs`, `cmap` (every code point 0..=0x10FFFF plus 0x110000 and 0xFFFFFFFF), `adv` (0..numGlyphs+16 and three larger ids), `kern` (every stored pair by key, keys past 2^32 as keys only, class 0 with the class definition in insertion order, `kernInLatin`), `twips` (`kernGlyphsTwips` at 6, 10.5, 11, 12 and 72 pt over every stored pair, 1,000 class-0 pairs and 10,000 seeded pairs; Rust replays the units with a test copy of the formula), `gsub` and `ligate` (every ligature as a sequence, 150 seeded sequences, 16 corpus words through the cmap, against `liga`, `liga+clig`, `liga+clig+dlig+hlig`, every feature tag, the empty set and an unknown tag), `vdmx` (0..=300; the TS also asks `p + 0.5`, which never answers), `colr`, `outline` (every glyph, numGlyphs, +1 and 0xFFFF), `symbol`, the converter `program` (open outcome, bbox, ascender, descender, italic angle bits, its own cmap sweep and advances; the TS copies in `pptx` and `xlsx` have identical code), `info` (`isFontFile`, `isEmbeddable`, `hasTrueTypeOutlines`, `faceInfo` with the names as code units), `gids` (`glyphIdsFor` over a 12,544-code-point block, the corpus text and 100 seeded sets), `subset` (`subsetTrueType` over all ids, the text's ids, {0}, ids at and past numGlyphs and 200 seeded sets, both `keepLayout` values, byte-exact digests), `eot` (both flags), `odttf` (3 keys, both directions) and `ttc` (`standaloneFace` of the font and of each probe). Per non-font input: 295 odttf keys, 119 style names, 266 language tags, the four variant combinations, the whole catalog tables, the 7 pattern files (id, minimums, every token) and `points`/`hyphenate(word, l, r)` for l, r in 1..=3 over 32,963 words (the oracle words, their upper-case and capitalized forms, every exception word) in 7 languages, 107 `loadHyphenation` lists (which pattern sets load), and **772 embedding scenarios** with a stub resolver (both apps, all settings, own file or not, document faces, already-embedded families, restricted/bitmap-only/editable licences, CFF, a cmap that fails when read, a cut directory, garbage, repeated families in different case, `İ`/`Σ` families): the sequence of `documentFace` calls (against `embedding_requests`: S13), of `loadFace` calls, and the plan (faces, subsetted flags, info digests, skipped reasons) or its rejection (27 rejected plans, all equal).

**Results** (`cargo test --release -p uo-fonts`, from `core/`):

| Replay | Inputs | Facets equal | Mismatches | Accepted by name |
|---|---|---|---|---|
| open + embedded fonts | 36 fonts | 759 | 0 | 0 |
| generated fonts + collections | 140 | 2,949 | 0 | 10 (2 G11 `face_info` budget on `gen/h/names_budget`, 1 G11 hang, 7 G4 counters) |
| mutants | 360 | 8,028 | 0 | 57 (1 G11 hang, 56 G4 counters) |
| odttf, style, lang, variants, catalog, hyphenation, embedding plan | see above | 5,129 | 0 | 3 (G9) |
| G16 legacy kern vs the C# reader (.NET 10.0.5) | 501 fonts (385 with pairs) | 501 | 0 | 0 |
| **Total** | | **17,366** | **0** | 70 |
| opt-in local capture (`C:\Windows\Fonts` 472 files + the Office cloud fonts 148, light depth) | 620 fonts, 605 open in the TS | 14,521 | 9, all one cause (below) | 0 |
| wasm self-test (`fonts-selftest`), Node 24.15 and WebKit 26.5 (Playwright) | 82 generated fonts (1,942 facets), 6 open fonts at full depth (144), 51 G16 fonts, 7 languages x 3 hyphenation chunks (28) | all equal in both engines | 0 | |

- **Accepted differences, by name, and what the capture counted.** G4 (lazy reads; the script writes the value the Rust must give and counts what it normalized in `<facet>.g4`): the TS throws on a read past the end during a query for 70 code points (generated fonts) and 698 (mutants) in `cmap`, 3 advances, and 6,570 in the converter program's cmap on mutants (the Rust gives 0); 1 + 11 outlines with an undefined coordinate (an end point past the last point), `None` in Rust; format-12 glyph ids above `u32::MAX` saturated (9 + 54 code points). **None of these occurs on a real font: the open, embedded and 620 local fonts have an empty G4 tally.** G11 (hostile input): 20 `subset` cases the TS never returns from on the S2 fonts (Rust gives the input back, asserted case by case), and two outlines on which the TS runs out of a 4 GiB heap (`gen/h/outline_explosion`, one mutant of Liberation Sans Bold: end points that re-emit the whole glyph for each pair), where Rust returns `None` at `max_outline_points`; `gen/h/names_budget` (5,400 name records pointing at one 16,383-character string: the TS builds 8.8 x 10^7 characters and answers, `face_info` stops at its work budget of 2^26 units: `info` and `eot` accepted by name); G9: 3 of the 107 `loadHyphenation` lists contain `constructor`, `toString` or `__proto__`, which the TS looks up in a plain object and rejects. Comparison is exact otherwise: no tolerance anywhere, floats by bits.
- **Finding for the maintainer (not a port bug): the work budget of `glyph_ids_for` trips on real fonts.** On 9 of the 620 local fonts (Meiryo ×5, Meiryo UI, Yu Mincho, HGP Gothic E, HGP Mincho E, UD Digi Kyokasho, whose format-4 cmaps have 14,335 and similar ranges) the `gids` facet over a 12,544-code-point block gives `Err(Limit(TableWork))` where the TS answers: its linear scan costs about 1.8 x 10^8 steps (0.3 s) and the budget is 2^26 = 6.7 x 10^7 units (one per segment visited per code point). With the budget lifted the facet equals the TS on all 9 (the local test requires it; the committed corpus pins the same cause with `gen/h/cmap_many_ranges`, 20,000 one-code-point segments). Consequence in the product: an embedding plan (and `subset`'s caller) that asks for more than about 5,000 to 9,000 distinct code points in such a font is rejected where the TS embeds it; a document that uses the family needs that many distinct characters only in the CJK case. The scan is linear on purpose (overlapping and unsorted segments make the first match order-dependent), so the options are to raise the budget of this one call (it is time, not memory: a cmap cannot make it allocate) or to accept the limit by name. Not changed: the budget values were approved at gate 1.
- **Not a mismatch but worth knowing.** The real fonts have no class-0 kerning (0 on the 22 + 620 here, as the skeptic measured): class 0 is covered by the generated fonts only (frontier C's 62, `gen/c`), which is exact.
- **Differences that were mine.** Two bugs in the capture script, both found by the first replay and fixed before any result above: `sfntToEot` wrote its tag before the call that threw (`O` then `E`), and the 500 ms guard on a `subsetTrueType` call mistook a slow case for the S2 loop (a mutant of Carlito Bold whose subset is 135 MB takes 0.36 s in the TS under no load); the guard is now 4 s + 2 ms per KB.

**Decision audit** (the porters' decisions outside the plan: phase 1: 8, frontier B: 15, C: 10, D: 16). *Equal* = verified equal against the TS by the facets named; *named* = a difference accepted by name or a typed-API restriction; *n/a* = no effect on results.

| # | Decision | Verdict | Evidence |
|---|---|---|---|
| P1 | whitespace table checked against V8, not uo-opc | equal | `gen_js.mjs` re-run on Node 24.15 / Unicode 17.0: `js_expected.txt` byte-identical (8,965 lines); the unit replay is in `cargo test` |
| P2 | data-model types declared in phase 1 | n/a | |
| P3 | trivial pure code translated in stubs (`Variant`, `variant_of`, ...) | equal | `variant/*`, `style/*`, `catalog` facets |
| P4 | `Tag::from_str` takes four Latin-1 characters | named | Latin-1 tags in `dir`/`subset` cases equal (`\xE9tag`); a feature string of another length cannot be asked in Rust (typed API), the TS matches nothing for it |
| P5, P7, P8 | placement, re-exports, `allow(dead_code)` | n/a | |
| P6 | `View::signed_offset` (hmtx index -1) | equal | `hmtx_zero` quirk font and mutants: `adv`, `program.adv` |
| C1 | `Face` wiring (`kern_units`, `kern_in_latin`, `ligate`) | equal | `kern`, `twips`, `ligate` through `Face` on 536 fonts |
| C2 | `ClassDef.order` (the TS `Map` order) | equal | class-0 entries with their class definition in insertion order, `gen/c` |
| C3 | coverage kept as runs | equal | coverage formats 1, 2, 3 in `gen/c`, real fonts, mutants |
| C4 | budget points as implemented | equal / named | no divergence on 536 + 620 fonts; the pinned counts are frontier C's tests; hostile outcomes are G11 |
| C5 | caps on stored class definitions and ligature records | named (G11) | no corpus font comes near them (the largest real GSUB, Gabriola, is 460 times under the wasm32 cap) |
| C6 | legacy `kern` reads a value only for an absent key | equal | `k_duplicate_value_cut`, `k_new_pair_value_cut`, `k_partial_failure` (TS `SfntFont`, `gen/c`); the C# reader is G16 |
| C7 | the Latin test runs inside the atomic GPOS read | equal | `g_script_feature_index_oob` (`kern`, `kernInLatin`) |
| C8 | `LigGroup.len` saturating add; `ligate(&[Tag])` | named | no TS counterpart above 2^32 groups; feature strings are four Latin-1 characters in practice |
| C9, C10 | differential design; not done | n/a | the libFuzzer targets and Miri are the security and memory verifiers'; the wasm run is below |
| D1 | `sfnt_to_eot` = `face_info` + layout | equal | `eot` facet, both flags, 536 fonts including the throws |
| D2 | EOT sizes wrap (mod 2^32, mod 2^16) | not verified | needs an input of 4 GiB; a name cannot pass 65,534 bytes, so the 2^16 wrap is unreachable; Rust tests only |
| D3 | the int32 rule of the subset | equal below 2 GiB, not verified above | all `subset` cases; the `len + 1 >= 2^31` branch needs a 2 GiB V8 allocation (frontier D's Rust test of the cap) |
| D4 | cap accounting of the subset | named (G11/G3) | no corpus font reaches the 256 MiB cap; a 135 MB output (a Carlito mutant) is equal |
| D5 | kept-glyph set (ids above `u16` ignored) | equal | sets with ids at and past numGlyphs (`[n, n+5, 0xFFFF, 0x10000]`) and 200 seeded sets with ids up to n+2 |
| D6, D11, D14, D15 | differential design, additive API, shared files, capture metadata | n/a | |
| D7 | odttf: the dash is gone before parsing | equal | 295 keys, 3 keys on every font, the 7 embedded fonts (`in` digest after `deobfuscate`) |
| D8 | catalog tables as an included file | equal | the whole tables in insertion order |
| D9 | hyphenation data format | equal | `fields` of the 7 files (minimums, every token), the TS `nfc` property is `true`, 32,963 words |
| D10 | hyphenator details (no NFC, root values inert, ASCII digits) | equal | 32,963 words x 7 languages, points and 9 `hyphenate` minimums each |
| D12 | `pattern_ids_to_load` own keys | equal / named (G9) | 107 lists; the 3 that the TS rejects are accepted by name |
| D13 | `embed.rs` shape (one walk, sorted code points, lower-cased policy) | equal | 772 scenarios; `embedding_requests` equals the `documentFace` sequence (a rejected plan stops after a prefix of it) |
| D16 | not done: TS differential of the plan, wasm run | done here | embedding scenarios; wasm below |
| B1, B15 | `cmap.rs` shape, tests | n/a | |
| B2 | cmap index building (stable sort, range cap after the reads) | equal | `cmap` sweep on 536 fonts (overlap, unsorted, odd `segX2`, arrays past the end, format 12); the cap is unreachable natively |
| B3 | arithmetic in `u64`/`i64` | equal | every `adv`, `outline`, `cmap` value |
| B4 | `advance_width_at` shared | equal | `adv`, `program.adv` |
| B5 | outline cache semantics | equal / n/a | glyphs at and past numGlyphs are `None` (`outline` covers n, n+1, 0xFFFF); the cache is transparent |
| B6 | outline limits (visits, two point totals) | named (G11) | 2 outlines where the TS exhausts 4 GiB; every other glyph of 536 + 620 fonts equal |
| B7, B8 | `FontProgram` shape, `legacy_kern` outcomes | equal | G16: 501 of 501 against the C# reader |
| B9 | budgets the plan did not list (`face_info`, the `.ttc` scan) | named (G11) | `gen/h/names_budget`; the `.ttc` scan budget is not reached by the 12 collections |
| B10 | S10 unobservable | equal | `info` on mutants with short OS/2 |
| B11 | `standalone_face` details | equal | `ttc` facet: bytes of the output for every probe of 12 collections, errors, and the input back for every plain font; the 2 GiB int32 wrap is not testable |
| B12 | `glyph_ids_for` details | equal / finding | `gids`: saturation (`gids.g4 sat`), other formats, empty set; the work budget is the finding above |
| B13 | the two accepted differences of the differential | confirmed | 12 outlines with an undefined coordinate and the saturation counted above (G4) |
| B14 | wiring of C's types | equal | as C1 |

**wasm.** `cargo build -p uo-wasm --features fonts-selftest --target wasm32-unknown-unknown --profile wasm` (369 KB module); the facets code of the native replay is included with `#[path]` (one copy). The module builds the 82 generated fonts itself and takes the real fonts and the pattern files from JS through `uo_alloc_bytes` buffers. `node --test crates/uo-wasm/tests/fonts_selftest.test.mjs` and `UO_PLAYWRIGHT=<playwright> UO_WASM_PATH=<wasm> node crates/uo-wasm/tests/browser/run.mjs --browser webkit crates/uo-wasm/tests/fonts_selftest.test.mjs`: all 5 tests pass in Node 24.15 and in WebKit 26.5 (the same file), 2,165 facets equal the capture (1.2 s and 1.5 s for the font tests). Not in wasm: the mutants, the embedding scenarios and the local capture (native only). CI: the wasm job of `.github/workflows/rust-core.yml` builds the feature and runs the file in Node and in WebKit (four lines added; the test skips itself when the module has no `fonts-selftest` exports, so the generic WebKit run over every test file does not fail on the other builds).

**Extended mutant campaign (opt-in, not committed).** 40 mutants per base instead of 8 (1,800 mutants; the seeds of the first 8 are the committed ones), TS heap 2 GiB: **40,156 facets equal, 0 unexplained mismatches, 294 accepted**: 11 outlines the TS runs out of memory on (all `outline`), 282 G4 counters (2,755 `cmap` throws, 2,551 advance throws, 9 `colr` throws, 60 undefined-coordinate outlines, 295 saturated ids in `gids`), and one `subset` difference that the replay accepts by rule (`OUTPUT_CAP`): a mutant of Liberation Sans Bold Italic whose `loca` makes the TS allocate a 537 MB subset, where the Rust refuses an output over `max_output_bytes` (256 MiB, D4) and gives the input back; the rule accepts a `subset` facet only when it equals the TS with the cap lifted, and it does (the output lengths of all 20 cases are equal too). A first run of 4,500 mutants was abandoned (the process was killed after repeated 4 GiB out-of-memory restarts of the TS worker) and is not counted. The capture script and the replay take `UO_FONTS_MUTANTS_PER_BASE`, `UO_FONTS_CAP` and `UO_FONTS_HEAP_MB`.

**Commands** (from `core/`, `CARGO_TARGET_DIR=core/target/fonts-parity`): `cargo test --release -p uo-fonts` (replay, seconds); recapture: `cargo run --release -p uo-fonts --example fonts_corpus -- <dir>`, then `UO_TS_ROOT=<built checkout of main> node crates/uo-fonts/tests/parity_support/capture_fonts.mjs --corpus <dir>` (about 3.5 minutes; two OOM restarts); G16: `cargo run --release -p uo-fonts --example legacy_kern_corpus -- <dir>` then `dotnet run crates/uo-fonts/tests/parity_support/legacy_kern.cs -- <dir> crates/uo-fonts/tests/parity/legacy_kern.cap`; local: `UO_LOCAL_FONTS=1 UO_LOCAL_FONTS_DIRS=<a>,<b> UO_LOCAL_FONTS_CAP=<cap> cargo test --release -p uo-fonts --test fonts_parity_local -- --nocapture` after `capture_fonts.mjs --dir <a>,<b> --out <cap>`.

**Rounds:** 0. No port bug was sent back; `core/crates/uo-fonts/src` is untouched by this stage. Maintainer's decision requested on the `glyph_ids_for` budget (finding above).

### Stage 4 — memory (measured)

Verifier: Sonnet, rust-port stage 4. Everything below comes from tests that ran, on the worktree at `c869abfb` plus the stage-4 fix F6 (see "Findings"). All sizes are **MiB** printed as "MB" (the 218 / 371 MB of frontier C in stage 3 are the 208 / 354 MiB measured here). Native numbers are `uo-alloc` per-thread counts on a release build (Windows x64, `lto`; the machine was shared with other agents, so times vary by up to 2×, bytes never do); wasm numbers are `uo_memory_live_bytes` and `memory.buffer.byteLength` of the `wasm` profile build (Node 24.15 and WebKit 26.5 through Playwright 1.61, the engine of Safari; real Safari 15 / iOS 15 is still gate 3).

How to run (from `core/`):

```text
cargo test --release -p uo-fonts --test stage4_memory -- --nocapture --test-threads=1      # M1, M2 (1,000 rounds, ~100 s), M3, M5, worst cases
cargo test --release -p uo-fonts --test stage4_memory -- --ignored --nocapture             # opt-in: C:/Windows/Fonts (UO_LOCAL_FONTS), numbers only
UO_TS_ROOT=<built main> node --expose-gc crates/uo-fonts/tests/memory_support/ts_budgets.mjs ../shared/fonts/files [extra .ttf]   # the TS reference
cargo build -p uo-wasm --features fonts-memtest --target wasm32-unknown-unknown --profile wasm
node --test crates/uo-wasm/tests/fonts_memory.test.mjs                                      # UO_FONTS_SOAK=<rounds> (default 300), UO_WASM_PATH
node crates/uo-wasm/tests/browser/run.mjs --browser webkit crates/uo-wasm/tests/fonts_memory.test.mjs
```

Files: `tests/stage4_memory.rs` (the tests), `tests/memory_support/mod.rs` (scenarios and hostile fonts; compiled by the native test **and** by `uo-wasm` under the feature `fonts-memtest`, so both measure the same code), `tests/memory_support/ts_budgets.mjs`, `uo-wasm/src/fonts_memtest.rs`, `uo-wasm/tests/fonts_memory.test.mjs`; CI: two steps in the `wasm` job of `rust-core.yml` (Node and WebKit, 300 rounds).

#### M1 — no leak per operation: pass

Stage 3 already had `assert_no_leak` over the generated fonts (140+ assertions in the foundation, frontier B, C and D tests: parse, every query, every error and limit path of the plan). Stage 4 adds the same over the **22 faces of the corpus** (the 16 open fonts and 6 generated ones) and the hyphenators, one operation at a time, outputs folded into returned values so a release build cannot drop the allocation:

| What | Test | Result |
|---|---|---|
| `Face::parse`, 200 outlines (cold and warm), measure of the oracle text with kerning, ligation of 200 words, COLR, VDMX, `kern_units`, `ligate`; `face_info`, `glyph_ids_for`, `FontProgram`, both subsets, EOT (both modes), odttf round trip, `standalone_face` (sfnt, a 2-face `.ttc`, a cut `.ttc`, a missing PostScript name), `embedding_requests` + `plan_font_embedding`, on each of the 22 faces | `m1_every_operation_on_every_real_and_generated_face_leaks_nothing` | live bytes return exactly |
| `Hyphenator::new`/`points`/`hyphenate`, `HyphenSession`, `HyphenationRegistry` (`register`, `for_lang`, `get`, `contains`, `ids`), `pattern_ids_to_load`, in the 7 languages | `m1_hyphenators_registry_and_sessions_of_the_seven_languages_leak_nothing` | exact |
| Limit and error paths on Carlito and Liberation Mono: `max_table_work` over 12 values (0 to 10^6), `max_kern_pairs` (5 values), `max_outline_work`, `max_outline_points`, `max_cmap_ranges`, `outline_cache_entries` 0/1/7, `max_output_bytes`; 51 truncations of each file (directory, tables, the middle, the tail) | `m1_limit_and_error_paths_over_real_fonts_leak_nothing` | exact |

Not repeated: the budget points of GPOS, GSUB and legacy `kern`, the `Nan` loop of the subsetter and the other limit paths on generated fonts, which the stage-3 tests already pin (`gpos_gsub_memory.rs`, `subset.rs`, `frontier_b_memory.rs`).

#### M2 — soak: pass

| Scenario | Rounds | Result |
|---|---|---|
| native *fresh* (0 B tolerance): parse the 22 faces → measure the oracle text with kerning → ligate 200 words → 200 outlines → COLR/VDMX → info, glyph ids, program → both subsets → EOT → odttf round trip → `.ttc` extraction ×2 → 7 hyphenators and sessions, 10,000 words → embedding requests + plan → drop | 1,000 (55 ms per round on a quiet machine) | live bytes 0 above the start at every sample (21 samples); `clear_caches()` is a no-op and leaves live unchanged |
| native *long-lived* engine (22 faces, 7 hyphenators and sessions kept; outline windows rotate so every outline cache fills past its cap, 10,000 new words per round) | 1,000 | sawtooth between 15 and 45 MB above the built engine (3.71 MB): the session caches fill to 50,001 words and clear; top of the second half 41.5 MB ≤ the caps (outline caches full of each face's 2,048 largest glyphs 39.7 MB + 7 full sessions 41.6 MB); last third peaks at 41.5 MB vs 42.9 MB in the middle (no trend, asserted within 4 MB); dropping the engine returns live to the start exactly |
| wasm *fresh*, Node 24.15 | 1,000 (47 ms per round) | live 0 above the start; `memory.buffer.byteLength` 12.75 MB from the first sample to the last (one distinct size) |
| wasm *long-lived*, Node | 1,000 | same sawtooth (13–42 MB); `byteLength` 56.4 → 58.6 MB (allocator fragmentation, asserted within 4 MB after the first half); engine dropped = start |
| wasm *fresh*, WebKit 26.5 | 1,000 (157 ms per round, 3.3× Node) | identical numbers: live 0, `byteLength` 12.75 MB flat |
| wasm *long-lived*, WebKit | 1,000 | identical to Node |

The corpus (16 open fonts, 6 generated, 7 pattern files and a `.ttc`) is 6.0 MB live in the module; `memory.buffer.byteLength` after loading it is 8.6 MB.

#### M3 — budgets: pass (first record; the +10% rule applies from here)

TS reference measured with `ts_budgets.mjs` (`--expose-gc`, frozen `dist` of `main`, heap + array buffers after 4 gcs, the input bytes held outside): retained size of a parsed `SfntFont` (its constructor reads GPOS and `kern`) and of a `Hyphenator`. Rust: `Face::parse` + first `kern_units` + first `ligate`, live bytes kept above the shared `Arc<[u8]>` of the file.

| Face | File KB | Glyphs | TS retained | Rust kept | Ratio | Rust parse + first kern |
|---|---|---|---|---|---|---|
| Carlito Regular / Bold / Italic / Bold Italic | 608–797 | 2,781–2,782 | 7.02–7.03 MB | **2.26 MB** (2,304 KB pair map, 9.4 KB GSUB, 1.4 KB face) | 3.1× | 5.1–6.0 ms (TS 28–51 ms) |
| Caladea (×4) | 57–60 | 420 | 0.13–0.18 MB | 0.037 MB (36 KB pairs) | 3.5–4.9× | 0.05–0.1 ms |
| Liberation Serif (×4) | 361–384 | ~2,600 | 0.02–0.06 MB | 0.015 MB | 1.4–4× | 0.01 ms |
| Liberation Mono (×4, no kerning) | 274–312 | ~2,424 | 0.03 MB | **2.0 KB** (target ≤ 64 KB) | 15× | 0 |
| Gabriola (742,163 pairs; opt-in, local) | 1,764 | 4,517 | 28.08 MB | **9.00 MB** (peak 13.6) | 3.1× | 24 ms (TS 285 ms) |
| Calibri / Bold / Italic / Z / Light / Light Italic (opt-in; 347K / 336K / 230K / 223K / 536K / 426K pairs) | 1,041–1,612 | 4,881–7,152 | 14.35 / 14.36 / 7.24 / 7.22 / 28.31 / 14.25 MB | 4.50 / 4.50 / 4.50 / 2.25 / 9.00 / 4.50 MB | 3.2× | 5–15 ms (TS 72–155 ms) |
| MS Gothic, Yu Gothic Regular, Microsoft YaHei, SimSun-ExtG (CJK, opt-in; 9.9–30K glyphs) | 3.4–18.7 MB | 9,852–30,209 | not measured (the TS `SfntFont` has no `.ttc`) | 0.8–56 KB beyond the file (compact cmap; kern 0 to 21K pairs, ≤ 0.3 MB) | | ≈ 0–1 ms |

Targets of the plan: Carlito ≤ 2.5 MB kept: **2.26 MB, met**; fonts without GPOS ≤ 64 KB: 2 KB, met (the CJK cmap index 19–56 KB, met); parse + first kern ≤ 3 ms native: **not met, 5.1–6.0 ms** on this machine (3 quiet runs). Memory is not the cause (it is 171,859 inserts into the pair map) and it is 5–9× faster than the TS; the target goes to the gate-3 benchmarks. Meiryo is not installed on this machine; the four CJK files stand in for the cmap case.

The pair map costs 18 B per pair at the cap (`FxHashMap<u32, i16>`: 9 B per bucket, 7/8 load, doubling) and 12.7 B per pair for Gabriola (742K pairs in 2^20 buckets).

Per operation on Carlito Bold (674 KB), native limits:

| Operation | Allocated | Peak above start | Kept |
|---|---|---|---|
| `Face::parse` | 2.8 KB | 1.4 KB | 1.4 KB |
| first `kern_units` (GPOS parse) | 5,154 KB | 3,506 KB | 2,304 KB |
| next `kern_units` | 0 | 0 | 0 |
| first `ligate` (GSUB parse) | 28 KB | 11 KB | 9.4 KB |
| measure the oracle text with kerning, 200 outlines | 1,903 KB | 307 KB | 300 KB (the outline cache) |
| `glyph_ids_for` (oracle text) / `face_info` / `FontProgram::parse` | 8 / 1.2 / 2.1 KB | 4 / 0.6 / 0.9 KB | same |
| `subset_true_type`, oracle text, keep layout / drop layout | 219 / 106 KB | 197 / 84 KB (output 161 / 46 KB; plan: ≤ 2 × output + loca) | |
| `sfnt_to_eot` (whole file) | 676 KB | 675 KB | the output |
| odttf obfuscate + deobfuscate | 1,349 KB | 1,349 KB | 0 |
| `standalone_face`: sfnt (borrows) / 2-face `.ttc` | 0 / 58 KB | 0 / 58 KB | 0 |
| `embedding_requests` + `plan_font_embedding` (2 variants, subset) | 103 KB | 51 KB | 0 |

The baselines of Carlito's first kern (peak 3.59 MB, allocated 5.28 MB) and of the de-1996 hyphenator (2.19 MB kept) are asserted at +10% by `m3_per_operation_allocated_and_peak_on_carlito_and_the_hyphenators`.

Hyphenators (kept after `Hyphenator::new`; TS = retained `Hyphenator`, the pattern data module in parentheses):

| Language | Pattern file | Rust kept (peak while building) | TS | Ratio |
|---|---|---|---|---|
| pt | 4 KB | 19 KB (45 KB) | 0.18 MB (+0.04) | 9.7× |
| it | 4 KB | 17 KB | 0.14 MB (+0.01) | 8.2× |
| fr | 11 KB | 116 KB (357 KB) | 0.79 MB (+0.04) | 7.0× |
| es | 42 KB | 250 KB (695 KB) | 2.04 MB (+0.08) | 8.3× |
| en-us | 32 KB | 423 KB (1.2 MB) | 2.32 MB (+0.06) | 5.6× |
| nl | 85 KB | 809 KB (2.3 MB) | 5.25 MB (+0.17) | 6.6× |
| de-1996 (36,709 patterns) | 274 KB | **2.14 MB** (5.7 MB) | 15.93 MB (+0.52) | 7.6× |
| all seven | | 3.68 MB | 26.7 MB (+0.9) | 7.2× |

Target of the plan: de-1996 ≤ 4 MB kept: met. A session keeps 70–120 B per cached word (3.5 MB for 50,001 words of 12 characters, 5.9 MB for 36 characters).

#### M5 — caches filled past their cap: pass, one fix (F6)

| Cache | Cap | Test | Result |
|---|---|---|---|
| outline cache per face, entries | 2,048 | `m5_the_outline_cache_is_bounded_past_its_entry_cap` | a cap of 100 over 2,000 glyphs keeps ≤ 132 KB; Carlito's 2,782 glyphs at the default cap keep 1.35 MB; a full walk of 13 local faces (Latin, CJK, emoji) keeps 0.6–3.2 MB |
| outline cache per face, **bytes** | none before F6 | `worst_case_of_one_outline_and_of_the_outline_cache` | a 326 KB font of composites kept 39 MB per cached glyph: **80 GB at the 2,048-entry cap** (F6). Now 4 MiB per face; 200 glyphs of 240 KB keep ≤ 3.89 MB; a glyph over 256 KiB of commands is returned and not cached |
| `HyphenSession` | 50,001 words | `m5_the_hyphen_session_cache_holds_at_its_cap_and_drops_with_the_session` (+ `hyphen_data.rs`) | 120,000 distinct words: never above 50,001 entries; kept ≤ 3.5 MB; dropped with the session, live exact. Bounded in bytes only through the document (a word is a slice of its text) |
| compact cmap index | `max_cmap_ranges` | `m5_the_compact_cmap_index_at_the_wasm32_cap_is_four_bytes_per_range` | 2^22 ranges (a 48 MB file): kept 16.00 MB exactly (4 B per range), peak 24 MB; the 2^22 + 1-th range is `Limit(CmapRanges)` with the same peak |
| pair map | `max_kern_pairs` | `m5_the_pair_map_kept_at_its_cap` | wasm32 cap 4,194,304 pairs: kept **72 MB** (the 75 MB of G5), peak **108 MB** (the table doubles: old and new buckets); native cap 16.8M: kept 288 MB, peak 432 MB |
| lazily parsed tables (GPOS, GSUB, VDMX, COLR) | the font, `max_table_work`, the stored-record caps of frontier C | `worst_case_per_limit_with_hostile_fonts` | see the next table |

The TS advance cache and GPOS `WeakMap` do not exist in Rust; the `FaceCache` of the native provider is in uo-docx-layout (S16).

#### Worst case per limit (hostile fonts; peak live bytes above the start, everything dropped afterwards)

| Hostile font | File | Native limits | wasm32 limits (native run) | In wasm (Node = WebKit) |
|---|---|---|---|---|
| 30,000 ligature sets → one 30,000-record set (GSUB, `ligate`) | 117 KB | 208 MB (1.3–2.1 s) | 52 MB | **28 MB** |
| 1,500 class-0 entries, each with a 65,536-entry class definition (GPOS, first `kern_units`) | 61 KB | 354 MB | 90 MB | **90 MB** |
| class matrix filling the pair map to its cap (4096² native, 2048² wasm32) | < 1 KB | 432 MB (kept 288) | 108 MB (kept 72) | **108 MB** |
| one pair past the cap (GPOS unreadable) | < 1 KB | 432 MB | 108 MB | |
| one composite glyph of 16 × a 65,535-point glyph (1,048,592 commands = 40 MB; `glyph_outline` after F6) | 326 KB | 120 MB (not cached) | 120 MB (`max_outline_points` is 2^20 on both) | **120 MB** |
| subset of 1,000 overlapping 60 KB glyphs (output 60 MB) | 60 KB | 57 MB | 57 MB (cap 64 MiB) | 57 MB |
| `.ttc` of 400 tables each pointing at the whole file (output 25 MB) | 60 KB | 25 MB | 25 MB | 25 MB |

`memory.buffer.byteLength` never shrinks: after the six hostile cases in one module it was 172 MB (58.6 MB after the long-lived soak); the high-water mark is the largest peak plus what stays live.

**Phone verdict.** Judged for an iOS tab (the limit varies by device and is not published; these are the numbers a hostile document can force, not measurements on a device):

- **The per-face limits are right for a desktop, not for a phone, and they are not summed.** A hostile document can embed N fonts of a few tens of KB, each costing its own 72–108 MB (pair map) or 90 MB (class-0 GPOS) for as long as its `Face` lives, and the wasm high-water mark is permanent. Nothing in `uo-fonts` bounds the total over the faces of a document; that is a policy of the layer that owns the faces (a budget of faces or kept bytes per document) and stays open for the layout and format crates.
- `WASM32.max_kern_pairs = 2^22` (kept 72 MB, peak 108 MB; it also caps the stored class definitions and ligature records of frontier C, 90 MB) is 5.7× the largest real font (Gabriola, 742,163 pairs, the maximum over 472 local faces). **Recommend 2^20 on wasm32** (1,048,576 pairs: kept 18 MB, peak about 27 MB, 1.4× Gabriola; the hostile GPOS about 23 MB): results stay equal to the TS for every real font, only a font with more than 1M pairs would read as unkerned. A maintainer decision (it moves a G5 number); not applied.
- `max_outline_points = 2^20` on both targets: one hostile `glyph_outline` peaks at **120 MB** (contours 24 B per point, commands 40 B per point, and the copy into the `Arc`), and the largest real outline over 472 faces is **1,200 commands** (46 KB), 870× under the limit. **Recommend 2^16 on wasm32** (peak about 7.5 MB, 54× the largest real glyph). A maintainer decision; not applied.
- `max_cmap_ranges = 2^22` (16 MB kept, 24 MB peak), `max_output_bytes = 64 MiB` (57 MB peak, one copy) and the `.ttc` output cap are acceptable on a phone as single transient allocations.
- Real use is far below all this: the 22 faces and 7 hyphenators live in 6 MB (shared file bytes included) plus at most 45 MB of caches, and the wasm module stays at 58 MB.

#### M4, M6

- **M4**: no handle crosses to JS or .NET yet (the bindings come with the format crates). The test-only wasm export takes byte buffers and returns numbers, as the plan says; `fonts_memory.test.mjs` also checks that loading the corpus and `uo_fonts_memtest_reset` bring the module's live bytes back to the start exactly.
- **M6**: `uo-fonts` is `#![forbid(unsafe_code)]`, so there is no block for Miri. The only `unsafe` of this stage is the buffer take-over of the test-only `uo-wasm/src/fonts_memtest.rs` (`Vec::from_raw_parts`, documented, the pattern of `uo_document_append_owned`). `dhat` was not needed: no growth was left unexplained.

#### Findings and fixes

- **F6, fixed: the outline cache was bounded by entries, not bytes.** `Face::glyph_outline` kept up to 2,048 outlines per face with no limit on their size. A composite of 16 components of one 65,535-point glyph is 130 bytes of file and 1,048,592 commands (40 MB); 2,048 of them fit in a 330 KB font, and a document that draws those glyphs keeps 80 GB (measured: 39 MB per glyph, 1,560 MB for 40). Fix (minimal, results unchanged): `OutlineCache` counts the bytes of its commands; the cache is cleared at 2,048 entries **or** 4 MiB (`OUTLINE_CACHE_MAX_BYTES`), and an outline over 256 KiB (`OUTLINE_CACHE_MAX_ENTRY_BYTES`, 6,500 commands) is returned without being cached. No real face is affected (a full walk of 13 local faces keeps ≤ 3.2 MB, under the cap). Regression: `worst_case_of_one_outline_and_of_the_outline_cache` (the 40 hostile glyphs keep 0 B; 200 glyphs of 240 KB keep ≤ 3.9 MB). Counted as rework of the Sonnet port (Process metrics, F6).
- **Open, for the maintainer**: the two phone recommendations (`WASM32.max_kern_pairs`, `max_outline_points`) and the missing per-document budget over faces.
- **Not memory**: parse + first kern is 5.1–6.0 ms against the plan's 3 ms (gate 3); `glyph_ids_for` over 196,608 code points on a large cmap stops at the work budget (`Limit(TableWork)`) by design (R-BUDGET on the linear TS scan).
- The M1 tests found no leak: every operation of every face returns to the exact start. The stage-3 numbers of frontier C reproduce (208 / 354 MiB native, 52 / 90 MiB with the wasm32 limits); in real wasm32 the GSUB case is 28 MB (smaller pointers), the others equal.

#### What is not measured

Real Safari 15 / iOS 15 and iPhone devices (WebKit 26.5 only; BrowserStack before gate 3); a document-level budget over many faces (no document layer yet); the C# facade and M4 handles (no bindings); the 1,000 rounds in CI (CI runs 300; 1,000 ran once in Node and WebKit); Meiryo (not installed); the TS memory of CJK fonts (the TS `SfntFont` does not read `.ttc`).

### Stage 4 — security (security specialist, Opus, 2026-11-02)

Review of every module of `src/` (`read`, `sfnt`, `cmap`, `face`, `gpos`, `gsub`, `outline`,
`vdmx`, `colr`, `program`, `info`, `ttc`, `subset`, `eot`, `odttf`, `catalog`, `variant`,
`hyphen/*`, `embed`, `js`) for panics, overflow of `u32`/`u64` offsets and `usize` on wasm32,
quadratic or unbounded work, allocations sized by declared counts, limit bypasses, and the
`Send`/`Sync` soundness of `Face`/`FontProgram`; fuzz targets in `core/fuzz`. Each finding below
was reproduced by a failing test before the fix (`tests/security_regressions.rs`). Inside the
limits the fixes change no result (the parity captures and every stage-3 test pass; three tests
that pin exact work units or point counts moved with the new counted points, named below).
`#![forbid(unsafe_code)]` holds, so there is nothing for Miri; `Face` and `FontProgram` are
`Send + Sync` by construction (`Arc`, `OnceLock`, a `Mutex` whose poisoning is recovered, no raw
pointer). `cargo audit` (RustSec, 1,402 advisories, 2026-11-02): no advisory for the 10 crates of
`core/` nor the 18 of `core/fuzz`. uo-fonts has no FFI or wasm export of its own yet (only the
test-only `fonts-memtest` of `uo-wasm`), so there is no binding boundary to review.

#### Findings (fixed; regression tests in `tests/security_regressions.rs`)

| # | Severity | Input | Effect before the fix | Fix | Test |
|---|---|---|---|---|---|
| S4-1 | high (DoS) | GPOS whose `kern` feature lists up to 65,535 lookup indices that all point at one lookup, whose up to 65,535 subtable offsets point at one subtable that reads nothing counted (PairPos format 3, an empty coverage, another lookup type, an extension to one) | the subtable loop was not a budget point (R-BUDGET counted lookup indices, not subtables): 4.3·10^9 visits outside `max_table_work`; 8,000 x 8,000 (a 48 KB table) took 0.48 s, the maximum (≈ 260 KB) ≈ 35 s native, more in wasm, on the first kerning query of the face. The TS loops the same way | one unit per subtable visited (`gpos.rs`) | `s4_1_gpos_subtable_visits_count_against_the_budget`; `gpos_kern::the_work_budget_counts_the_named_points` now 57 units (was 56: one subtable) |
| S4-2 | high (DoS) | the same shape in GSUB (type-4 subtables that are not format 1, extension lookups to one) | the same, on the first `ligate` of the face | one unit per subtable visited (`gsub.rs`) | `s4_2_gsub_subtable_visits_count_against_the_budget`; `gsub_ligate::the_work_budget_counts_the_named_points` now 23 units (was 21: two subtables) |
| S4-3 | high (DoS) | a GSUB whose ligature list for one first glyph holds millions of entries (262 subtables x 32,000 records pointing at one ligature: a 64 KB table that fits the default parse budget and the store cap, 8.4 million candidates) | `applyLigatures` compares every candidate at every position of the run and the query had no budget: `ligate` of 100 glyphs took 1.5 s, of 1,000 glyphs ≈ 15 s, for every call the layout makes. The TS has the same loop | `GsubLigatures::ligate_within(gids, features, &mut WorkBudget)`: one unit per group visited by each lookup, per candidate examined and per component compared; `Face::ligate` uses the face's `max_table_work`, `GsubLigatures::ligate`/`apply` the default. Over the budget the run gets no ligatures (`None`, what an unreadable GSUB gives; `apply` returns the groups unchanged). Real text costs groups x lookups x a few candidates | `s4_3_ligate_work_is_bounded_by_the_budget` |
| S4-4 | high (abort) | a simple glyph with 32,767 end points all 0 (65 KB once), drawn through composites | an end point before the start makes an empty contour that no limit counted (`max_outline_points` counted points only): 64 x 32,767 empty contours = 50 MB peak for one composite level; two levels (4,032 visits within `max_outline_work`) make 1.3·10^8 contours, ≈ 3.2 GB on the way up: the wasm instance aborts, the native host swaps | an empty contour counts one against `max_outline_points` (`outline.rs`), so the contours of a call are bounded like its points | `s4_4_empty_contours_count_against_the_point_limit`; `outline::emitted_points_count_against_the_limit_even_when_ends_repeat_points` now 8 (was 7: one empty contour) |
| S4-5 (= F6) | high (memory) | composites of 16 copies of a 65,536-point glyph (512 bytes): 2^20 points per outline | the outline cache was capped by entries only: 2,048 outlines of 42 MB each | found by the memory verifier as well and fixed there (F6: 4 MiB per face, outlines over 256 KiB not kept) | `s4_5_outline_cache_is_bounded_in_commands` (passes with F6), `stage4_memory.rs` |

The four fixes are on Sonnet code that followed the plan where the plan had the gap (S4-1, S4-2:
the R-BUDGET list; S4-4: the S1 point limit) or had no rule (S4-3: the plan budgeted the GSUB
parse, not the query); they are counted as rework below (F7–F10) for the model comparison.

#### Checked, no finding

- Panics: no `unwrap`/`expect`/indexing on font data in `src/` (clippy `unwrap_used`/`expect_used`
  deny; slices through `get`); `count.ilog2()` of the subset runs on at least four tables.
- Arithmetic: every offset is `u64` built from `u32`/`u16` values plus bounded products (the
  largest is `(c1 * c2n + c2) * rec_size < 2^37`); every `u64 → usize` is `try_from`; the outline
  accumulates in `i64`; the binary searches run in `i64`; EOT sizes wrap as `setUint32` does. No
  overflow in the fuzz builds (overflow checks on).
- Declared counts: nothing is allocated from a count of the file except `loca_out` of the subset
  (`(numGlyphs + 1) x 4` ≤ 256 KB, after every offset was read) and the `with_capacity` of the
  outline coordinates (≤ the flags read). The cmap index, pair map, class definitions, GSUB store,
  directory, name strings, `.ttc` and subset outputs are bounded by what is read or by a limit
  checked before allocating.
- Work: every loop is bounded by a `u16` count of reads that fail past the end, by a budget point
  (with S4-1 to S4-3), or is linear in the input (cmap parse, embedding walk, odttf, catalog;
  `glyph_ids_for` is budgeted per segment visited). `kern_in_latin`, VDMX, COLR and the legacy
  `kern` (`length` 0 re-reads one subtable: budgeted per subtable) are fine. Recursion: only the
  outline, depth ≤ 9.
- Hyphenation: `points` is O(letters x the longest pattern); the shipped patterns are trusted
  (longest 9 letters), words are untrusted and cost linear time. The session cache is bounded in
  words (50,000) and in bytes by the document text it was given.
- `glyph_ids_for` can fail a legitimate plan: P1.

#### Proposals for gate 2 (behaviour changes, not applied)

| # | Evidence | Recommendation |
|---|---|---|
| P1 — `glyph_ids_for` budget on CJK | the embedding plan's cmap reader scans segments/groups linearly per code point, one unit each, under one `max_table_work` (2^26) per call. Measured (opt-in `p1_glyph_ids_for_budget_on_local_cjk_fonts`, `C:\Windows\Fonts`, read only): **Yu Gothic fails from 10,505 distinct kanji, Malgun Gothic with all of its 27,133 characters** (`Err(Limit(TableWork))` fails the whole plan); asked for every character they map, 6 of the 454 local faces exceed the budget (Malgun Gothic regular and bold, Yu Gothic in four weights) | keep the TS answers, not its cost: when the end codes (format 4) or the groups (format 12) are sorted and do not overlap (checked once, O(n)), the first match in table order is the binary-search match, so search; scan only unsorted subtables (stage 5, results equal) |
| P2 — ligatures without a budget outcome | S4-3 bounds each `ligate` call (2^26 units, 0.1–0.3 s) but not a document that calls it once per run | index the ligatures of each (lookup, first glyph) as a trie of the component sequences that keeps the lowest font index at each end: the first match of the TS is the lowest index met while walking the following glyphs, so the cost is the run length x the depth, results equal (stage 5); the S4-3 budget then never trips |
| P3 — `max_outline_points` | the largest real outline over 454 local faces has 996 segments (Segoe UI Symbol; Calibri 831). The default 2^20 lets a 512-byte glyph and a composite cost 2^20 points per call (25–50 MB transient, ≈ 30 ms), and F6 no longer caches outlines over 256 KiB, so every paint of such a glyph pays it again | 2^16 (65 times the real maximum): a sixteenth of the time and memory per call; parity unchanged (no real or corpus glyph comes near) |
| P4 — limits per document | every cap is per face and per call; a document embedding n hostile faces keeps n x (pair map + class definitions + GSUB store), 50–100 MB per face with the wasm32 limits (memory record: 94 MiB GPOS, 52 MiB GSUB) | a document-level byte budget in the layout provider's `FaceCache` (uo-docx-layout plan, S16), past which a face is rejected; until then the shell embeds few faces from untrusted documents |

#### Fuzzing

17 targets in `core/fuzz/src/fonts.rs` (plain `fn(&[u8])`, wrapped for libFuzzer by
`fuzz_targets/fonts_*.rs`; the stable driver runs them from `TARGETS`): `fonts_parse` (any bytes,
`Face::parse` and every query: cmap, advances, kerning, `ligate` covering its input, VDMX, COLR,
outlines well formed `(M (L|Q)* Z)*` and cached identically), `fonts_sfnt_dir` (`TableDir`,
`table_of`, the `Map` order), `fonts_cmap` (a hostile `cmap` through the face, the converter
program and `glyph_ids_for`: sorted, distinct, no 0), `fonts_gpos` (a hostile GPOS or legacy
`kern`: within the pair and class caps, `kern_units` equal to a model over the parsed pairs and
class 0), `fonts_gsub` (`ligate`/`apply` cover the run, deterministic), `fonts_outline` (hostile
`glyf`/`loca`/`numGlyphs`, both loca formats), `fonts_colr_vdmx`, `fonts_program` (with the C#-only
legacy `kern`), `fonts_info` (`face_info`, licence, outline test, `glyph_ids_for`), `fonts_ttc`
(`standalone_face` by name: the output within the cap, every table inside it, re-parsed and
queried), `fonts_subset` (the input back, or an output within the cap whose kept glyphs hold the
input's bytes), `fonts_eot` (sizes, version, magic, the font at the end), `fonts_odttf`
(involution, only the first 32 bytes change, the key against an independent `parseInt` model,
UTF-16 keys with lone surrogates), `fonts_catalog`, `fonts_hyphen_build` (arbitrary patterns,
exceptions and words: points inside the word and sorted, `hyphenate` within the minimums, the
session equal to the hyphenator, the file format round trip), `fonts_hyphen_words` (the seven
shipped hyphenators), `fonts_embed` (the plan asks for exactly the requested faces, in order;
faces answered with the base font, a hostile font or nothing). Every public call runs under small
limits (2^16 units of work, 4,096 pairs, 256 outline visits, 2^14 points, 1 MiB of output; one
selector in eight tighter still) and must finish within 1 s, so a loop outside the budget is a
finding long before it hangs. Seeds (`--bin seed`, `fonts_seeds`): a base font built in code with
every table the crate reads (cmap formats 4 and 12, glyf/loca in both formats with every composite
form, GPOS pair formats 1 and 2 and an extension, GSUB ligatures and singles, legacy `kern`,
COLR/CPAL, VDMX, name), a two-face collection, the shapes of S4-1, S4-2 and S4-4, and the 16 open
fonts of `shared/fonts/files` subsetted to 48 glyphs plus their GPOS, GSUB, cmap and `kern` tables
(at most 64 KB each). The CI fuzz job runs every file of `fuzz_targets` (now 33) for 60 s.

| Target | libFuzzer execs (25 min) | Corpus after | Stable driver execs (60 s) |
|---|---|---|---|
| fonts_parse | 180,099 | 1,290 | 715,213 |
| fonts_sfnt_dir | 14,891,505 | 200 | 6,419,365 |
| fonts_cmap | 621,987 | 616 | 607,591 |
| fonts_gpos | 130,011 | 689 | 273,273 |
| fonts_gsub | 302,514 | 779 | 714,017 |
| fonts_outline | 649,816 | 486 | 898,771 |
| fonts_colr_vdmx | 3,364,003 | 244 | 1,458,447 |
| fonts_program | 600,550 | 655 | 1,226,788 |
| fonts_info | 3,058,314 | 583 | 8,477,000 |
| fonts_ttc | 539,491 | 1,329 | 4,565,800 |
| fonts_subset | 1,481,499 | 374 | 2,181,070 |
| fonts_eot | 5,192,778 | 291 | 3,440,107 |
| fonts_odttf | 9,122,768 | 475 | 8,984,783 |
| fonts_catalog | 4,977,964 | 416 | 5,340,088 |
| fonts_hyphen_build | 569,544 | 1,192 | 1,117,895 |
| fonts_hyphen_words | 534,317 | 748 | 534,509 |
| fonts_embed | 331,110 | 1,104 | 1,547,796 |

libFuzzer: cargo-fuzz 0.13.2, nightly, WSL Ubuntu 24.04, AddressSanitizer, overflow checks and
debug assertions on, `-timeout=10 -rss_limit_mb=2048`, the 17 targets in parallel for 1,500 s
each: 46.5 million executions, **0 crashes, 0 timeouts, 0 out-of-memory** (peak RSS 440–614 MB).
The targets that take whole fonts run at 100–1,000 executions/s under ASan with inputs up to
64 KB, hence their lower counts. The stable driver (no coverage, mutations of the same corpus,
then of the libFuzzer corpus) ran 48.5 million executions with no crash and no call over its 2 s
slow mark (slowest 377 ms, `fonts_hyphen_build`). Its first pass found one false invariant of the
subset target (glyph 0 checked in a font that declares no glyphs), fixed in the target, not in
the crate.
After the last edits of the targets (JS white space as the hyphenation separator, clippy) a final
5-minute libFuzzer pass of the 17 targets on the final build ran 9.9 million more executions,
0 crashes.

## Evolution

Candidates for stage 5 (results must stay equal unless listed separately): class-based kerning
instead of the expanded pair map (spike lesson 3: parse time and ~1–2 MB per Latin face), one
cmap reader (G6), one table-directory rule, the outline cache policy.

| Change | Keeps results? | Before | After | Decision |
|---|---|---|---|---|
| Outline cache bounded by bytes (4 MiB per face, outlines over 256 KiB not kept), stage-4 fix F6 | yes (a miss decodes the glyph again) | up to 80 GB from a 330 KB font | 4 MiB per face | done |

## Final review and benchmarks (gate 3 — pending)

## Process metrics

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| 1 | planner (Opus) | 21.8 min | 59 |
| 1 | reuse specialist (Opus) | 8.7 min | 58 |
| 2 | analysis skeptic (Opus) | 14.8 min | 55 |
| 1 (revision) | planner (Opus): S1–S19 written in, consolidated gate-1 list | 7.6 min | — |
| 3 (phase 1) | porter: foundation (Sonnet) | 17.7 min | 88 |
| 3 (phase 2) | porter: frontier B (Sonnet) | 49.2 min | 133 |
| 3 (phase 2) | porter: frontier C (Sonnet) | 36.9 min | 86 |
| 3 (phase 2) | porter: frontier D (Sonnet) | 41.6 min | 134 |
| 4 | memory verifier (Sonnet) | ≈ 45 min | ≈ 115 |
| 4 | security specialist (Opus) | ≈ 115 min | ≈ 100 |
| 4 | parity verifier (Sonnet) | ≈ 160 min | ≈ 180 |

Stage 3 wall time ≈ 67 min (phase 1, then the three frontiers in parallel; B and D were
restarted once after an API usage limit, the times above are the runs that did the work), against
≈ 33 min for uo-opc's Opus port. Times are model output speed and scope together (uo-fonts is the
bigger port and its frontiers wrote their own TS differentials), so they compare only roughly.

Stage-3 integration by the orchestrator (rework, for the model comparison):

| # | What | Why | Fixed by |
|---|---|---|---|
| F1 | `js.rs` tripped the new bans (a float→int cast, `parse::<f64>` in the V8 oracle test) | the uo-jsnum ban list landed during the port | orchestrator: reasoned `allow` on ToUint8; oracle regenerated as IEEE bits (21,094 values identical) |
| F2 | `catalog.rs` used `str::trim_start` (banned) | same | frontier D, after a note |
| F3 | `Face` wiring of C's kerning/ligatures sat between two frontiers | the plan gave the API, not who wires it | frontier B, after a note |
| F4 | `NotPorted` placeholder and its assertions left in | expected end of the stub phase | orchestrator |
| F5 | 5 files with CRLF (frontier C) and mixed endings in the record | Windows editing (the uo-opc R4 lesson again) | orchestrator / frontier B |
| F6 | the outline cache was capped in entries only: a 330 KB font of composites could make it keep 80 GB | frontier B built the cache as the plan said (clear at 2,048 entries); the plan bounded neither the size of an outline nor the bytes of the cache | memory verifier (Sonnet): 4 MiB per face, outlines over 256 KiB not cached, regression in `stage4_memory.rs` |
| F7 | GPOS subtable visits outside the work budget (S4-1) | the R-BUDGET list of the plan omitted the subtable loop; the port followed it | security specialist (Opus): one unit per subtable |
| F8 | the same in GSUB (S4-2) | same | security specialist |
| F9 | `ligate` work unbounded per call (S4-3) | the plan budgeted the GSUB parse, not the query | security specialist: `ligate_within` with the face budget |
| F10 | empty contours outside `max_outline_points` (S4-4) | the S1 point limit counted points only | security specialist: an empty contour counts one |

Decisions taken outside the plan by the Sonnet porters: phase 1: 8, frontier B: 15, frontier C:
10, frontier D: 16 (each listed in its subsection). The largest: frontier C capped stored kerning
and ligature records at `max_kern_pairs` because the work budget bounded time but not memory (a
120 KB GSUB font reached 872 MB) — a memory design decision the plan had missed.

## Model comparison: uo-fonts (Sonnet port) vs uo-opc (Opus port), through stage 4

The maintainer asked for this comparison before using Sonnet for the next modules. Both modules
had Opus planners and skeptics; uo-opc's port and verifiers were Opus, uo-fonts' port and its
parity/memory verifiers were Sonnet (security Opus in both). uo-jsnum increment A (Sonnet port,
Opus pow) is listed as a second Sonnet data point.

| | uo-opc (Opus) | uo-fonts (Sonnet) | uo-jsnum A (Sonnet) |
|---|---|---|---|
| Library code (non-comment lines) | 5,150 | 4,374 | 2,275 |
| Stage-3 wall time | ≈ 33 min | ≈ 67 min | ≈ 25 min (+ pow, Opus) |
| Stage-3 agent time (sum) | 63 min | 145 min | — (interrupted once) |
| Decisions outside the plan, declared | not tracked | 49 (all audited: verified or accepted by name) | 12 (all audited) |
| Decisions outside the plan, **not** declared | — | 0 found | 1 (J1: `saturating_add` where V8 wraps) |
| **Parity rounds** (port bugs the parity verifier sent back) | **0** | **0** | **0** |
| Stage-4 defects found in the port (memory + security) | 7 (F1–F7) | 5 (F6 memory; F7–F10 security) | 1 (J1) |
| …of which caused by a gap in the (Opus) plan rather than the translation | F1 inherited from fflate; F2–F5 quadratic paths | F7–F10 (budget points the plan missed); F6 (cache bounded by count only) | — |
| Integration rework by the orchestrator | 4 (generators, error class, re-export, CRLF) | 5 (new bans, wiring between frontiers, stub removal, CRLF ×2) | 1 (escape in an opt-in) |
| Stage-4 verifier time | parity 54.5, memory 35.3, security 85.5 min | parity 84.6, memory 47.2 (Sonnet), security 63.3 min (Opus) | parity 50.2, memory 11.7 (Sonnet), security 83.0 min (Opus) |
| Verifier findings in the verifiers' own work | — | — | the port's "0 B" test discarded its result (memory verifier); a coverage gap in rem_pio2 (parity verifier) |

Reading: on closed-scope modules the Sonnet ports were as faithful as the Opus port (0 parity
rounds in both Sonnet modules, verified by corpus captures the orchestrator regenerated
byte-identical), and found about as many defects in stage 4, all but one rooted in plan gaps. The
costs are wall time (about 2× here, partly because the Sonnet frontiers also wrote their own TS
differentials) and one silent deviation (J1) — a decision not declared is the risk to watch, not
translation accuracy.

## Gate 2 — decisions (approved 2026-11-03)

Maintainer, verbatim: "Segue com os 2" — every recommendation accepted (D1–D3 now, D4 in the uo-docx-layout plan, D5 in stage 5).

Parity 0 mismatches (17,366 facets + the C# G16 capture), memory gate green (M1–M5), security
F7–F10 fixed, 105e6 fuzz execs without crashes. Proposals from the verifiers:

| id | Decision | Recommendation |
|---|---|---|
| D1 | `glyph_ids_for` work budget trips on 9 of 620 real CJK fonts (Meiryo, Yu Mincho…) where the TS embeds (security P1 = parity finding): replace the linear scan by a binary search when the cmap ranges are sorted and disjoint (same results), and keep the budget for the rest | approve |
| D2 | wasm32 `max_kern_pairs` 2^22 → 2^20 (≈ 18 MB kept / 27 MB peak; 1.4× the largest real font, Gabriola) | approve |
| D3 | `max_outline_points` 2^20 → 2^16 (largest real outline over 472 faces: ~1,200 commands; one hostile glyph peaks at 120 MB today) | approve |
| D4 | a per-document byte budget over faces in the layout provider's face cache (uo-docx-layout), since every cap here is per face | approve, in the uo-docx-layout plan |
| D5 | ligature trie (linear ligate, same results) | stage 5 |
