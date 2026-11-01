# uo-fonts — font files, OpenType kerning and ligatures, embedding, hyphenation

Record of the rust-port workflow. Status: **gate 1 approved 2026-11-01** (G1–G16 as the final recommendations); next: stage 3 (Sonnet port, modules 0–17).

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

## Verification (gate 2 — pending)

### Parity

### Memory (measured)

| Check | Result | Numbers |
|---|---|---|
| M1 no leak per operation | | |
| M2 soak (native, fresh / long-lived) | | |
| M2 soak (wasm) | | |
| M3 budgets | | |
| M4 boundaries | | |
| M5 cache caps | | |
| M6 Miri / LeakSanitizer | | |

### Security

## Evolution

Candidates for stage 5 (results must stay equal unless listed separately): class-based kerning
instead of the expanded pair map (spike lesson 3: parse time and ~1–2 MB per Latin face), one
cmap reader (G6), one table-directory rule, the outline cache policy.

| Change | Keeps results? | Before | After | Decision |
|---|---|---|---|---|

## Final review and benchmarks (gate 3 — pending)

## Process metrics

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| 1 | planner (Opus) | 21.8 min | 59 |
| 1 | reuse specialist (Opus) | 8.7 min | 58 |
| 2 | analysis skeptic (Opus) | 14.8 min | 55 |
| 1 (revision) | planner (Opus): S1–S19 written in, consolidated gate-1 list | 7.6 min | — |

Parity rounds and rework: none yet.
