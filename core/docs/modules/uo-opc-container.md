# uo-opc — the document container layer

Record of the rust-port workflow. Status: **gate 3 approved and implemented; uo-opc done for 0.10** (gate 3 approved 2026-11-01: the stage-6 findings, the numbers, and R1–R5 — R1, R2, R3, R5 as proposed; R4 as the final skeptic revised it, default level 4 with `level` keeping its miniz meaning; implemented 2026-11-02, see "Gate 3 decisions — implemented").

## Scope

- **TS covered**:
  - the fflate subset in use (`unzipSync`, `zipSync`, `inflateSync`, `unzlibSync`, `zlibSync`,
    streaming `Inflate`/`Unzlib` of `officeart.ts` `inflateCapped`);
  - the package classes `DocxPackage` (write), `XlsxPackage` (sheet), `PptxPackage` (slides),
    `OdfPackage`/`writeZip` (write odf);
  - the three XML parsers (`xml.ts` of docx-core, sheet-core, slides-core) and their helpers;
  - the CFB reader (three identical copies: doc, xls, ppt);
  - generic XML writing (`doc/xmlw.ts`, escapes) and the raw surgery of `lossless.ts`
    (`splitElement`, `setKid`, `withAttrs`);
  - `.rels` reading and `relsPathFor` (duplicated in sheet, slides, docx and `shared/fonts/src/ooxml.ts`);
  - the zip writers of XPS (`editor/src/xps.ts`), `.doc → .docx` (`doc/docx-writer.ts`), ODS and
    ODT (STORED `mimetype` written first).
- **C# covered** (moves to Rust): `OpcPartName.ResolveRelationshipTarget`/`EscapeSegment`,
  `OpcPackageLimits`, `OfficeLimitExceededException`, and the strict reading of the Editing API
  (`DocxDocument`: System.Xml, DTD prohibited, depth 256, close tags checked, namespaces resolved).
- **Stays in the format crates**: property order tables, namespace tables, the canonical-prefix
  table (passed in as data), `[Content_Types].xml` editing (on the `ContentTypes` type from here),
  `.ppt → .pptx` conversion, the field limits of `DocxLimits`.
- **Converter**: its stale copies of `xml.ts`/`package.ts` are replaced by presets of the shared
  parser; the parity reference for each product is the app's own copy (see decisions).

## Plan (planner + reuse specialist, revised after the skeptic)

- **One crate `uo-opc`**: `zip`, `flate`, `cfb`, `xml::{read, write, raw}`,
  `opc::{package, part_name, rels, content_types}`, `limits`, `error`. `flate` is also used by
  `uo-pdf` and the image decoders (PNG, TIFF, BLIP); `cfb` serves .doc/.xls/.ppt.
- **XML**: one iterative parser (explicit stack, no recursion) with options; presets per format
  live in the format crates: prefixes (`Canonical(table)` per scope for docx and charts,
  `StripRootMain` for xlsx, `Literal` for pptx), EOL normalization (xlsx), policy for bad
  character references and processing instructions, depth, raw capture, optional source positions.
  Plus a **strict mode** (namespace-resolved, close tags checked, DTD prohibited, depth 256) for
  the Editing API and the sanitizer.
- **XML tree, compact**: arena with interned element/attribute names, values and texts as spans
  into the source with a side table only for decoded/normalized strings, positions only when asked
  (docx lossless save). The source is an owned `Arc<str>`, no lifetime parameter, so models can
  keep raw slices without copying.
- **Package, lazy**: the input bytes are retained; parts are `Compressed{range, method, crc, size}`
  until read, inflated on demand with the limits applied then; untouched entries are copied raw on
  save; `retain` for bulk removal.
- **Inflate**: own port of fflate's `inflt` (same acceptance set on corrupt input: no NLEN check,
  lenient Huffman trees, Adler/CRC not checked), with the caps on top.
- **Deflate**: `miniz_oxide`, pinned; parity on decompressed bytes (see decisions).
- **Zip**: own port of fflate's container semantics (EOCD window, zip64, methods 0/8, Latin-1 and
  UTF-8 names, last-wins duplicates), never preallocating from declared sizes; fixed mtime.
- **CFB**: own port of `cfb.ts` (lenient by design); the `cfb` crate only as a dev-dependency for
  fixtures.
- **Relationships**: one resolver (the C# rule) and one ASCII case-folding function, exported for
  hosts (the clickCompliance validator must check exactly the part the library opens).
- `#![forbid(unsafe_code)]`; small crates allowed: `memchr`, `indexmap`, `miniz_oxide`.

### Parity

- Capture from the frozen TS (`uo-testkit`), hashed canonical streams on both sides (not JSON):
  package load (ordered names, sizes, hashes, or error class), XML trees per preset (positions
  converted UTF-16 → UTF-8 with a prefix table), CFB entries and stream hashes, relationship
  resolution **outcome** per document and rId (part found or not), save read back by fflate,
  .NET and Rust (names, order, method, flag 0x800, decompressed bytes).
- Corpus: 2,742 oracle docx, sheet and slides samples, legacy .doc/.xls/.ppt fixtures, **plus a
  generated quirk corpus** (names without flag 0x800, duplicates, size lies, backslashes, UTF-16
  parts, percent-encoded targets, corrupt deflate streams) — the Word files exercise none of them.
- Differential corpora against fflate (inflate acceptance) and the C# resolver (`%FF`, `%2e%2e`).

### Memory (measured gate)

- M1 on every operation including error paths (truncated zip, limit hit mid-inflate, depth error).
- M2: 1,000 × (open docx/xlsx/pptx/ppt → parse every XML part → edit → save → reopen → drop), tolerance
  0 B (no caches), wasm `byteLength` flat after warm-up.
- M3: budgets on `giant-1m.xlsx` (31.7 MB sheet XML), the largest oracle `document.xml` and a
  generated deck with large media; references: TS (heap + ArrayBuffers) and C# allocated bytes.
- M5: the caseless index and the CFB mini stream.

### Security

Zip (EOCD window, entry count ≤ len/46, offsets checked, no preallocation, methods 0/8, hostile names
on write: >65,535 bytes, >65,535 entries or 4 GiB need zip64), CFB (all TS guards in u64), XML
(iterative, depth, no DTD/entity expansion, saturating numeric references, hashed duplicate-attribute
lookup), relationship targets (C# rule). Fuzz targets: zip open/roundtrip/write with hostile names,
CFB, XML per preset and strict, escape roundtrip, inflate, rels and content types, raw surgery,
caseless index, part text decoding, relationship resolver.

## Analysis skeptic (verified in the code)

| # | Finding | Blocking | Proposed decision |
|---|---|---|---|
| 1 | fflate inflate accepts corrupt streams that miniz_oxide rejects (no NLEN check, browser.js:253; lenient trees) — images/parts would load or fail differently | yes | port `inflt` (G3) |
| 2 | Zip byte parity is pointless (fflate stamps `Date.now()`); PDF parity is already structural in `PdfParityTests` | no | parity on decompressed bytes (G2) |
| 3 | Eager inflate keeps input + all parts for the life of a wasm tab; a 256 MiB part limit refuses a pptx with a 300 MB video that TS opens | yes | lazy package, limits per part kind (G4) |
| 4 | Planned arena ≈ 9× a sheet cell's source; `Cow<'s>` forces copies in the models | yes | compact arena, `Arc<str>` source |
| 5 | No strict XML mode for Editing/sanitizer (C# uses System.Xml: DTD prohibited, depth 256) | yes | strict mode in this module (G6) |
| 6 | `&#xD83D;&#xDE00;` is a valid emoji in JS (verified); mapping each half to U+FFFD loses data | yes-ish | combine pairs (parity); lone halves → U+FFFD (TS writes U+FFFD bytes on save anyway) |
| 7 | Resolver, caseless lookup, UTF-16 parts, depth, Slides filter, backslash names change what opens/appears (Write resolves `word/${target}` without normalization, document.ts:2189) | yes | per-app policies reproduce TS during parity; flip in stage 5 with oracle evidence (G5) |
| 8 | Missed users: converter copies, XPS/.doc/ODS/ODT zip writers, `shared/fonts` rels, `shared/charts` `canonicalNs` | no | added to scope |
| 9 | Capture as JSON hits V8 limits; offsets need a prefix table; the oracle exercises no container quirks | no | hashed streams + generated corpus |
| 10 | Missing fuzz targets; C#↔Rust resolver differential | no | added |

## Decisions for gate 1 (approved 2026-11-01)

Maintainer, verbatim: "Aprovar G1–G7" — all seven recommendations accepted.


- **G1** one crate `uo-opc` (cfb, flate as modules) — recommended.
- **G2** deflate with `miniz_oxide`; parity on decompressed bytes (zips part-level, PDF streams
  decompressed) — recommended over porting fflate's deflate.
- **G3** inflate: own port of fflate's `inflt` — recommended.
- **G4** lazy package (retained input, inflate on demand, raw copy on save); limits per part kind:
  XML parts 256 MiB, binary parts up to 2 GiB like TS — recommended.
- **G5** behaviour changes: reproduce TS per app during parity, flip in stage 5 with evidence;
  except two immediate fixes that are not parity targets: the Sheet parser hang on unclosed
  comments/tags becomes an error, and `&constructor;` stays literal — recommended.
- **G6** strict XML mode now, for Editing and the sanitizer — recommended.
- **G7** parity reference: each app's own copy (the converter's stale copies are replaced) —
  recommended.

## Stage 3 — port (done 2026-11-01)

Modules: `flate` (fflate `inflt` port + miniz_oxide deflate), `zip`, `package` (lazy), `cfb`,
`xml::{read, tree, write, raw, entities, chars}`, `rels`, `content_types`, `part_name`, `limits`,
`error`. About 5,300 lines; `#![forbid(unsafe_code)]`; dependencies: `miniz_oxide` only.
`cargo clippy --workspace --all-targets --all-features -D warnings` clean; 149 workspace tests pass
in debug and release.

### Evidence (re-run by the orchestrator, not only reported)

| Check | Reference | Result |
|---|---|---|
| CFB reader (`UO_TS_PARITY=1 … cfb_ts_parity`) | the three TS copies (doc, xls, ppt), 713 generated files (578 open, 135 rejected) | identical, every entry field and stream hash |
| XML lenient presets (`xml_ts_oracle`) | frozen 0.9.x dist parsers (docx, sheet, slides, charts), 4,757 cases | 2,054 equal trees, 2,687 equal failures; the 16 differences are the G5 fixes; oracle regenerated from main's dist byte-identical to the committed file |
| XML strict mode (`xml_strict`) | real .NET `XmlReader` with `DocxDocument.Parse` settings, 929 inputs | identical; oracle regenerated byte-identical |
| Inflate | fflate 0.8.3: 18 fixtures + 4,000 mutated-stream results | identical |
| Zip read | fflate `unzipSync`, 20 quirk archives | identical; stored output byte-identical to `zipSync` with the fixed time |
| M1 | `assert_no_leak` on every public operation and error path of every module | pass |
| M3 (XML tree) | 100k-cell sheet, 3.88 MB source | 8.48 MB kept (40.4 B/element, 2.18× source; asserted < 44 B/element and < 2.4×) |

### Decisions taken in the port

- Fixed zip time 2000-01-01 00:00 (the value `to-pptx.ts` already used).
- Lazy parts: one `OnceCell` per entry, released by `release`/`release_all`, `set_part`,
  `remove_part` or drop; stored entries are slices of the retained input. Reads take `&self`;
  `Package` is `Send`, not `Sync`. `verify()` is the eager check matching the TS load.
- The total budget counts each entry once, on first read (stored entries included, as in C#).
- Zip guards the TS lacks: entry count above `len/46` is `Corrupt`, above `max_entries` is
  `Limit{Entries}`; a written name over 65,535 bytes is `Error::NameTooLong`.
- Strict mode follows `XmlReader` also on `xml:space` (only `default`/`preserve`, found by the
  .NET oracle).

### Open for stage 4 / gate 2

- **Security (proposal S1):** an empty deflate stream with a declared size returns that many zeros
  (fflate `new u8(declared)`), up to the part cap — a tiny file can make the library allocate
  2 GiB per part (4 GiB total). In JS that is a catchable RangeError; in Rust an allocation failure
  aborts the process (the .NET host included). Recommendation: an empty stream with declared
  size > 0 is `Corrupt` (no real document has one), a behaviour change to be approved at gate 2.
- Fuzz targets (zip, CFB, XML per preset and strict, inflate, rels, content types, raw surgery,
  resolver) and the capture/replay parity over the 2,742-sample oracle corpus in `uo-testkit`.
- M2 soak and M3 budgets at package level; inflate throughput (bounds-checked bit reads).
- `Limits::max_xml_depth` is applied by the format crates when they build `ParseOptions`.
- Known divergences kept: lone surrogates cannot exist in Rust strings (`stripInvalid` in slides
  and docx behave the same); `with_attrs` matches names literally (TS built an unescaped regex);
  rels/content-type insertions escape values (some docx call sites inserted them raw).
- `from_parts` keeps insertion order; Slides' `Record`-based `fromParts` applies
  `zip::js_object_order` first.

## Stage 4 — memory (memory verifier, 2026-11-01)

All numbers come from tests that ran; allocation counts of `uo-alloc` are deterministic (the
same in debug and release, measured equal on both). A reallocation counts as a new allocation of
the new size, so "allocated" includes every growth step of a `Vec`.

### M1 — no leak per operation

The stage-3 tests (`package_memory`, `zip_memory`, `flate_memory`, `cfb_memory`, `xml_memory`)
left out part of the public API; `tests/api_memory.rs` adds it: part names (`resolve`,
`escape_segment`, `fold_ascii`, `is_xml_like`, `part_cap`), every rels helper, the content-type
queries and edits, `join_kids`/`set_kid`/`insert_child` (all `Insert` modes)/`expand_self_closing`/
`remove_range`/`with_attrs`, every escape preset and `XmlWriter` method, the tree accessors (strict
`attr_ns` included), UTF-8/Latin-1/UTF-16 decoding, `adler32`/`crc32_update`/`inflate_stream`,
the CFB queries, the package queries under the three `CaseFold` policies; error paths: broken rels,
content types and raw edits (parse failing mid-tree), save with a name over 65,535 bytes after
other entries, stored save and `verify` of a damaged stream, not-a-zip. **Pass**: live bytes
return exactly (0 B) for every operation.

### M2 — soak (tolerance 0 B, the crate has no caches)

Scenario per iteration (`tests/memory_support`): `justify4/sw-16-8.docx` (largest oracle
`document.xml`), `emwidth/ew-emb.docx` (embedded font), `sheet/samples/showcase.xlsx` (charts,
VML), `slides/samples/stress-media.pptx` (stored media), each: open → read every part and parse
every XML part with the app's preset (docx with positions and raw capture, sheet/charts, slides) →
edit (element appended to the main part, relationship added, content-type override added, new
part) → save → reopen → `verify` → read and parse everything again → drop; plus
`legacy/lo-reference.ppt`: open the compound file and read every stream.

| Run | Iterations | Live bytes sampled every 50 | max − min | After `clear_caches()` | Result |
|---|---|---|---|---|---|
| native `m2_soak_100` (every `cargo test`, 19 s debug) | 100 | 771,247 B ×3 | 0 B | = start | pass |
| native `m2_soak_1000` (`#[ignore]`: 190 s debug, 19 s release) | 1,000 | 771,375 B ×21 | 0 B | = start | pass |
| wasm (`uo-wasm/tests/opc_memory.test.mjs`, 16–18 s) | 1,000 | 0 B ×21 (module live bytes) | 0 B | — | pass |
| wasm `memory.buffer.byteLength` | 1,000 | 2,621,440 B ×21 | 0 B | high-water mark after warm-up (one pass): **2,621,440 B** (2,490,368 B on a build before the concurrent stage-4 changes to `src`; wasm grows in 64 KiB pages) | pass |

The baseline (771 KB) is the input files held by the test. The wasm path is a test-only export
behind the `uo-wasm` feature `opc-memtest` (`src/opc_memtest.rs`,
`uo_opc_memtest_round_trip(ptr, len, kind)`, the same round trip on a buffer from
`uo_alloc_bytes`, with reduced copies of the presets); it is not the binding API. The CI wasm job
builds with `--features count,opc-memtest` and runs the test.

### M3 — budgets (`tests/package_budgets.rs`, asserted with +10%)

Rust: `uo-alloc` (`measure`); "kept" = live bytes after, outputs alive. TS: frozen 0.9.x dist
(`memory_support/ts_budgets.mjs`, one node process per scenario): "retained" = heapUsed +
arrayBuffers after GC with the outputs alive; "peak RSS" = growth of the process's maximum RSS (an
upper bound; untouched zero pages are not resident, so the deck's 64 MiB of zeros shows less). C#:
Core `OpcPackage` + `MiniXml` (`memory_support/cs_budgets.cs`): `GC.GetAllocatedBytesForCurrentThread`,
retained by `GC.GetTotalMemory(true)`; no per-operation peak in .NET.

| Operation | Rust allocated | Rust peak | Rust kept | TS retained | TS peak RSS | C# allocated | C# retained |
|---|---|---|---|---|---|---|---|
| `giant-1m.xlsx` open + read all + parse all XML (31.7 MB sheet; trees dropped per part) | 355,417,216 | 146,112,471 | 34,570,059 | 31,881,904 | 822,595,584 | 902,162,312 | 31,721,544 |
| `giant-1m.xlsx` parse `sheet1.xml` alone (2,010,005 elements) | 259,725,808 | 111,542,160 | tree 68,360,900 (2.16× source) | — | — | — | — |
| `giant-1m.xlsx` save untouched (Rust: raw copy; TS/C#: deflate everything) | 8,549,574 | 5,696,218 | output 2,848,247 (capacity 5,695,682) | 3,086,375 | 4,829,184 | 12,778,480 | 2,844,640 |
| `giant-1m.xlsx` save with `sheet1.xml` deflated again | 24,687,877 | 18,716,138 | output 2,856,819 | — | — | — | — |
| `sw-16-8.docx` open + read + parse `document.xml` (188,270 B, 6,657 elements, positions) | 2,310,825 | 821,642 | 714,170 | 2,755,447 | 5,369,856 | 3,668,960 | 2,604,992 |
| `sw-16-8.docx` edit + save (the Rust edit also parses the part and edits rels and content types: 4 deflated entries) | 4,293,381 | 896,035 | output 5,395 | 278,481 | 5,320,704 | 1,348,392 | 5,424 |
| deck (`reference.pptx` + 64 MiB deflated zeros + 16 MiB stored video, 17.2 MB archive) `open_owned` + parse all XML | 1,187,259 | 146,128 | 117,190 (+ the archive moved in) | 84,182,187 | 69,632,000 | 295,125,848 | 84,003,632 |
| deck edit + save | 54,282,370 | 34,477,829 | output 17,236,397 (capacity 34,465,528) | 17,146,645 | 42,045,440 | 84,049,200 | 16,887,440 |
| deck read the 64 MiB deflated part (contrast) | 228,153,002 | 81,156,608 | 67,108,864 | — | — | — | — |
| deck read the 16 MiB stored part | 9 (lowercased extension) | — | 0 (slice of the source) | — | — | — | — |
| one deflated entry of 4 bytes (miniz_oxide state, per `ZipWriter::add`) | 253,624 | 253,622 | — | — | — | — | — |

Lazy package, proven with numbers: after opening the deck, parsing every XML part, editing a slide
and saving, `produced_bytes` stays at the XML parts (105,780 B; asserted < 300 KB); the media are
never inflated (that read alone costs 228 MB allocated, 81 MB peak) and the save copies them raw
into the output only (allocated < 4× output, peak < 2× output, both asserted). TS and C# hold
every part inflated (84 MB) and deflate the 64 MiB again on every save (TS 1,078 ms).

Observations for stage 5 (not leaks, bounded, numbers above):
- **Save output capacity up to 2× its length**: `ZipWriter` grows `out` by doubling and returns it
  as is (giant: 5.70 MB capacity for 2.85 MB; deck: 34.5 MB for 17.2 MB); the caller holds the
  slack, and in wasm it raises the high-water mark. The raw sizes are known before writing, so an
  exact `reserve` (or `shrink_to_fit`) removes it.
- **Inflate with a declared size grows past it**: the output starts at 3× the input and doubles
  (64 MiB part: 80 MiB capacity, then `shrink_to_fit` copies to 64 MiB — 228 MB allocated, 81 MB
  peak). Clamping each growth step to `min(declared, cap)` keeps "never preallocate from the
  declared size" and bounds the peak by the output.
- **`part_text` → `parse(&str)` copies the source** into the tree's `Arc<str>` (31.7 MB for the
  giant sheet); handing the inflated bytes over as the source would save one copy per part.
- **miniz_oxide allocates ~254 KB of compressor state per deflated entry** (`deflate_raw` per
  `add`); a writer-level reusable compressor would remove it from small edits.

### M4 — boundaries

Not applicable yet: uo-opc has no handle crossing to JS or .NET (no `uo-wasm`/`uo-ffi` export
besides the test-only `opc-memtest`, which takes a byte buffer and returns a number). The M4 tests
arrive with the binding API.

### M5 — caches (`tests/cache_caps.rs`)

uo-opc has no global or cross-call cache; what it keeps lives inside one object and is bounded by
an input limit:

| Cache / index | Bound | Filled past it | Measured |
|---|---|---|---|
| `Package` caseless index (`CaseFold` fallback) | one entry per part; an opened archive has ≤ `max_entries` parts | `max_entries + 1` entries → `Limit{Entries}`; 100,000 lookups of distinct missing names → live bytes unchanged; `remove_part` drops it | 4,000 parts: 377,266 B (names 106,890 B); 4,000 names folding to one key: 270,384 B (table pre-sized by part count) |
| `Package` inflated parts (`OnceCell` per entry) | `max_total_bytes` over first reads, `part_cap` per part | 40 × 100 KB parts with a 1,000,000 B total: the 11th read is `Limit{TotalBytes}`; 20 rounds of `release_all` + re-read hold the same bytes, `produced_bytes` stays 1,000,000 | held 1,000,000 B |
| `CfbFile` mini stream + path index | the file length | root and a stream declaring 2^40 + 2^32 − 1 bytes in a 32 KB file; 1,000 re-reads | allocated 19,424 B, mini stream 4,096 B; flat |
| `xml::Document` interned names | distinct names of one document (≤ source length) | 50,000 elements per case | repeated name 3.77× source; 100,001 distinct names 8.56×; 50,001 shortest distinct names 17.13×; minimal `<a/>` elements 7.00× (asserted +10%) |

Not a leak, but a size to know for gate 2 (security): the tree is linear in its source with a
worst case of ~17× (short distinct names) and ~7× (minimal elements), against 2.2× for real
sheets. With the 256 MiB XML part cap, one hostile part can hold several GB of tree. A cap on
elements or on distinct names per document would bound it to real-document sizes.

Parser-local tables (`dedupe`, namespace `scope`) and the zip reader's duplicate index are freed
when the call returns (M1).

### M6 — tools

`#![forbid(unsafe_code)]`: Miri is optional. Run as Linux on a subset
(`cargo +nightly miri test -p uo-opc --target x86_64-unknown-linux-gnu --test api_memory --test
content_types_ops --test rels_ops --test xml_raw --test xml_write --test zip_write`): 29 tests pass
(`api_memory` 8 in 45 s, `content_types_ops` 2, `rels_ops` 4, `xml_raw` 4, `xml_write` 4,
`zip_write` 7) with no undefined behaviour and no leak reported at exit; `zip_write`'s
`zip64_when_more_than_65535_entries` was stopped after 14 minutes (too slow interpreted, not a
failure). Deflate- and corpus-heavy tests are not practical under Miri; with no `unsafe` in the
crate, the counting allocator's M1/M2 checks are the leak evidence.
LeakSanitizer runs the whole workspace on the Linux CI (`leak-sanitizer` job), these tests
included. dhat was not needed: no growth was found.

### S1 measured (behaviour unchanged; `tests/s1_empty_deflate.rs`)

| Crafted archive | Read | Allocated | Live |
|---|---|---|---|
| 132 B, `word/document.xml` declaring 256 MiB (the XML cap) | `part()` | 268,435,468 B | 256 MiB of zeros |
| 132 B, declaring 256 MiB + 1 | `part()` → `Limit{PartBytes}` | 29 B | — |
| two XML parts declaring 64 MiB | `verify()` | 134,217,738 B | peak 64 MiB |
| 340 B, three binary parts declaring 2 GiB each (`#[ignore]`, run by hand) | `part()` × 3 | 2,147,483,653 B + 2,147,483,653 B, the third `Limit{TotalBytes}` | **4 GiB held** |
| same entries, Slides open (`skip_impossible_ratio`) | — | entries over max(1 MiB, 1,100 × compressed) are left out | ≤ 1 MiB per entry |
| wasm, 132 B docx declaring a 256 MiB `document.xml`, app round trip | read + parse | the zeros plus the parser's copy of the source | `byteLength` +537,001,984 B, **permanent** (wasm memory never shrinks); module live bytes return to 0 |

Amplification: ~2,000,000:1 for an XML part, ~12,600,000:1 for 4 GiB from 340 bytes. The tests
measure the current behaviour and, if S1 is fixed (empty stream with a declared size → `Corrupt`),
check that nothing is allocated instead.

### Commands

```
cargo test -p uo-opc                                   # M1, M2 (100), M3, M5, S1
cargo test -p uo-opc --release --test package_soak -- --ignored                   # M2, 1,000
cargo test -p uo-opc --release --test s1_empty_deflate -- --ignored --nocapture   # S1, 4 GiB
UO_M3_RECORD=1 cargo test -p uo-opc --test package_budgets -- --nocapture         # new budgets
cargo build -p uo-wasm --features count,opc-memtest --target wasm32-unknown-unknown --profile wasm
node --test crates/uo-wasm/tests/opc_memory.test.mjs   # M2 and S1 in wasm
UO_TS_ROOT=<frozen 0.9.x checkout> node --expose-gc crates/uo-opc/tests/memory_support/ts_budgets.mjs
dotnet run crates/uo-opc/tests/memory_support/cs_budgets.cs
```

## Stage 4 — parity (parity verifier, 2026-11-01)

### Tooling: `uo-testkit` (new crate, no dependencies)

- `stream`: hashed canonical streams (`Stream`: `tag`, `u32`, `u64`, `i64`, `f64` by bits, `bool`,
  `str`, `bytes`, `digest_of`; SHA-256 of the item sequence; optional readable trace), the same
  encoding as `js/testkit.mjs` (pinned by a digest computed by Node in `tests/kit.rs`).
- `capture`: the capture file (`key<TAB>facet=value…`, `@meta`), `Capture::{read, parse, subset}`,
  and `Replay` (`check`, `accept` with a named reason, `skip`, `finish` → `Report` with equal,
  mismatches, accepted, never-replayed records; `assert_clean`).
- `generate`: `Rng` (the xorshift32 of the JS generators) and `build_zip` (raw archives with any
  flag, method, size, offset, local name, zip64 record or entry count).
- `zipdir`: an independent, strict central directory listing (names as bytes, flags, methods).
- `js/testkit.mjs`: `Stream`, `CaptureWriter`, `tsRoot` (`UO_TS_ROOT`/`--ts-root`), `importFrom`,
  `walk`, `utf8Offsets` (UTF-16 index → UTF-8 offset prefix table), `Rng`.

### Capture and replay

- Capture (manual, needs a built checkout of main): `UO_TS_ROOT=<checkout> node
  core/crates/uo-opc/tests/parity_support/capture_opc.mjs` → `tests/parity/opc.cap` (1.27 MB,
  3,142 records, 128-bit digests; byte-identical on re-run; TS commit in `@ts-commit`). It runs
  `cargo run --example opc_quirk_corpus` for the generated corpus.
- Replay (every `cargo test`, ~50 s debug): `tests/opc_parity.rs`, 8 tests over disjoint shares of
  the capture plus a coverage check. Each app runs its own code (G7); the per-app policies
  (open options, main part, presets per part, rels scope, relationship filter, target rule,
  lookup) are in `tests/parity_support/mod.rs` as what the format crates will own.
- Opt-in cross readers (`UO_TS_PARITY=1 UO_TS_ROOT=…`, `UO_DOTNET_PARITY=1`):
  `save_cross_readers` writes the Rust saves (raw and rewritten), has the TS save every package,
  and reads all of them back with fflate, uo-opc and .NET `ZipArchive`.
- Trace of one input: `UO_PARITY_TRACE=<key> cargo test -p uo-opc --test opc_parity -- --nocapture`
  against `capture_opc.mjs --only <key> --trace <key>`.

Facets per input and app (Write `w`, Sheet `s`, Slides `p`): `in` (input digest), `load` (ordered
part names + bytes, or the fflate error class / missing main part), `xml` (every XML-like part
with the app's presets: Write `docx-pos` with UTF-8 positions and raw capture; Sheet `sheet`, plus
`sheet-chart` (`canonicalNs`) for `xl/charts/` and `xl/drawings/`; Slides `slides`), `rels` (every
relationship in the app's scope: presence of `Id`/`Target`, external, the app's target rule, found
exact/caseless/missing), `save` (the saved archive: raw names, flag 0x800, method, decompressed
bytes; TS `zipSync` read by fflate, Rust read by `zipdir` + uo-opc), `edit` (set an existing part,
remove one and set it again, add a new and an integer-like name, save), `cfb.doc/.xls/.ppt` (the
three TS readers: every entry, `has`, every stream).

### Results: 0 mismatches

| Corpus | Inputs | Packages loaded | Parts | XML trees | Rels parts / relationships | Saves read back | CFB files |
|---|---|---|---|---|---|---|---|
| docx-layout oracle | 2,742 docx | 2,742 | 17,798 | 17,292 | 3,027 / 6,681 | 10,968 | — |
| sheet/samples | 7 xlsx | 7 | 80 | 86 | 19 / 55 | 28 | — |
| slides/samples | 35 pptx, 25 .ppt | 35 | 1,860 | 1,759 | 738 / 2,068 | 140 | 25 |
| other fixtures (site, dotnet, converter, docx-core, viewer, ppt-lab) | 35 OOXML, 11 .doc | 35 | 665 | 638 | 150 / 498 | 140 | 11 |
| generated quirks (`parity_support/quirks.rs`) | 239 archives × 3 apps, 48 embedded CFB | 532 | 10,880 | 8,514 (+851 errors on both sides) | 2,253 / 26,548 | 2,128 | 48 |

Every facet equal, or accepted by name:
- **G4** (approved): 324 raw saves keep the source method of untouched stored entries (`zipSync`
  deflates everything); names, flags and bytes equal, and the rewritten save equal with methods.
- **G5** (approved): 3 sheet-core hangs and 6 sheet-core garbage trees (slides-core fails on the
  same text) where uo-opc reports an unclosed construct; the capture lists them (`g5.s`).

Cross readers: 10,053 saved archives (3,351 packages × Rust raw, Rust rewritten, TS `zipSync`)
read identically by fflate and uo-opc (names, flags, methods, bytes) and, except the 9 below, by
.NET `ZipArchive` (names and bytes).

The quirk corpus covers: names without flag 0x800 (Latin-1), flag 0x800 on invalid UTF-8 and on a
BOM, duplicates, integer-like and `__proto__` names, lying sizes (short, long, stored, empty deflate
with a declared size — S1 — and an impossible ratio for the Slides filter), data ranges past the
end, backslashes, case differences (Sheet main part), UTF-16 and BOM-prefixed parts, invalid UTF-8,
percent-encoded names and targets, 27 odd relationship targets per relationship part, missing main
parts, method 12, zip64 with and without the extra field, comments, truncated archives, no EOCD,
lying entry counts, a bad local offset, a different local name, 60 corrupt deflate streams, 150
seeded random combinations (removals, duplicates, reorders, stored/deflated, XML quirk tokens,
truncations, flag flips, renamed parts), unclosed constructs, and 48 embedded compound files
(three .doc fixtures and 45 mutations of their header, FAT and directory).

### Differences found, not approved (behaviour unchanged, for the maintainer)

- **P1 — `__proto__` followed by numeric names** (`quirk/005-proto-then-numeric-names`, 3 apps):
  after an entry named `__proto__`, fflate's result object has that entry's Uint8Array as
  prototype, and a typed array's [[Set]] silently ignores canonical numeric names that are not
  valid indices of it (`-1`, `1.5`, `-0`, `NaN`…; ECMA-262 TypedArray [[Set]] with receiver ≠
  target), so the TS loses those parts; uo-opc keeps them. The replay reproduces the drop in the
  test (`js_proto_dropped`) and accepts it only when that explains the whole difference
  (`PENDING`). Recommendation: keep uo-opc's behaviour (a G5-style fix, like `&constructor;`).
- **P2 — raw copy of malformed entries** (consequence of G4, opt-in .NET check): a raw save
  copies a source entry as is, with its lying declared size (`quirk/006-size-lies`), its data read
  through a bad local offset (`quirk/025-bad-offset`) or a deflate stream only fflate's lenient
  inflate accepts (`quirk/038-corrupt-deflate-9`: .NET throws `InvalidDataException`). fflate and
  uo-opc read the copy as they read the source, but other readers do not, while the TS save
  (everything deflated again) is well-formed. Real files: none. Recommendation: copy raw only
  entries whose read output matched the declared size (and CRC when known); otherwise deflate the
  read bytes again.

### Stage-5 evidence (not parity targets)

Relationships kept by an app rule whose found/missing outcome differs under the C# rule
(`resolve_relationship_target` + ASCII case folding): real files 2 (`site/assets/samples/demo.docx`:
Write resolves `../customXml/item1.xml` to `word/../customXml/item1.xml`, missing; the C# rule
finds `customXml/item1.xml`), quirks 2,853 (`./`, `%20`, `#frag`, `..`, case, backslashes).

### Not verified here

- No `.xls` exists in the repository: the sheet CFB reader ran on the .doc/.ppt files and the
  embedded mutations only. No OOXML fixture embeds an OLE compound file (the 48 come from quirks).
- Not captured over the corpus: the ODF package and `writeZip` (ODS/ODT/XPS/`.doc → .docx` zip
  writers), `[Content_Types].xml` and `.rels` editing, raw surgery (`lossless.ts`), the strict mode
  (stage 3 compared it with .NET on 929 inputs), Write's other relationship rules
  (`numberingImages` normalizes `..`; fonts, `partByRelType`) and `shared/fonts` `relsPathOf`.
- Write parts are compared with positions everywhere (a superset of the calls without them).
- Sheet charts use `sheet-chart` only under `xl/charts/` and `xl/drawings/`.
- .NET compares names and bytes only (`ZipArchive` exposes no flags or methods).
- S1: the corpus has an empty deflate stream with a declared size (`quirk/006-size-lies`); if S1
  is approved (`Corrupt` there), the replay will report it and the decision must be named in
  `opc_parity.rs`.

## Stage 4 — browsers (2026-11-01)

Baseline Safari 15 / iOS 15 (rust-port skill). The default `wasm32-unknown-unknown` features
(bulk memory, multivalue, mutable globals, non-trapping float-to-int, reference types, sign
extension) are all in Safari 15; CI validates the `.wasm` against that set with `wasm-tools`
(with a SIMD module as the negative case) and runs the wasm tests in WebKit
(`crates/uo-wasm/tests/browser/run.mjs`).

Minimal check in real Safari on BrowserStack (`crates/uo-wasm/tests/browser/safari/`, exposed
through a temporary anonymous dev tunnel, deleted after the run): instantiate the 234 KB build
(`count,opc-memtest`), 10,000 document handles created and freed, then the uo-opc round trip
(open, parse every XML part, edit, save, reopen, verify) on a docx, an xlsx, a pptx and a .ppt,
200 iterations.

| Browser | SIMD in the browser | Handles: live bytes | Round trip ×200 | Live bytes | Wasm memory |
|---|---|---|---|---|---|
| WebKit 26.5 (Playwright, Windows) | yes | 0 → 0 | 3.3 s | flat 0 | flat 2,621,440 B |
| Safari 15.6.1, macOS Monterey | **no** | 0 → 0 | 5.1 s | flat 0 | flat 2,621,440 B |
| Safari 15.5, iPhone 13, iOS 15.5 | **no** | 0 → 0 | 3.1 s | flat 0 | flat 2,621,440 B |
| Safari 26.5, iPhone 17, iOS 26.5 | yes | 0 → 0 | 1.5 s | flat 0 | flat 2,621,440 B |

All pass. The floor browsers have no wasm SIMD, which confirms the baseline rule. Not covered yet
(full run before gate 3): the S1 input and the large files on a phone, low-RAM devices, and
Lockdown Mode (disables WebAssembly; product decision pending).

## Stage 4 — security (security specialist, 2026-11-01)

Review of every module of `src/` (zip, flate, package, cfb, xml, rels, content types, part names,
limits) for panics, allocations sized by declared values, quadratic or unbounded loops, recursion,
limit bypasses, name handling, the relationship resolver and integer truncation on wasm32; fuzz
targets in `core/fuzz`. Each finding below was reproduced by a failing test before the fix; every
fix keeps the results identical to the frozen TS (tables, outputs, errors of real documents).
`#![forbid(unsafe_code)]` holds, so there is nothing for Miri. `cargo audit` (RustSec, 1,395
advisories, 2026-11-01): no advisory for the 7 crates of `core/` nor the 14 of `core/fuzz`.

### Findings (fixed; regression tests in `tests/security_regressions.rs`)

| # | Severity | Input | Effect before the fix | Fix | Test |
|---|---|---|---|---|---|
| F1 | high (DoS) | DEFLATE dynamic blocks with an over-subscribed literal tree (35 bytes per block) | fflate's `hMap` loop is O(symbols × 2^15) per table: 2.1 million writes per block; a 105 KB stream took 11.4 s (release), a 50 MB part ≈ 1.5 h. The TS has the same loop | `h_map` keeps the literal loop when it makes at most 2 × 2^mb writes (every complete tree: same allocations, M3 unchanged) and builds the same table by ranges, last to first with a skip list, above that: O(2^mb + symbols) | `oversubscribed_trees_inflate_in_linear_time`; `flate::tests::h_map_matches_the_literal_port` (3,006 trees, both paths against the literal port) |
| F2 | medium (DoS) | `rels::remove_rels` on a part with many relationships | one `remove_range` (a full copy) per removed relationship: 80,000 relationships (5.4 MB) ≈ 60 s | one pass over the ranges | `remove_rels_is_linear` |
| F3 | low | `rels::first_free_rel_id` with n ids taken | every candidate compared with every id: 150,000 ids ≈ 50 s; `n += 1` overflowed (debug panic) at `u64::MAX` | hashed ids, wrapping increment | `first_free_rel_id_is_linear` |
| F4 | low | `raw::set_kid` on an element with many repeated kids | one `Vec::remove` per duplicate: 250,000 kids ≈ 47 s | one `retain` | `set_kid_with_many_duplicates_is_linear` |
| F5 | medium (DoS) | `raw::expand_self_closing` (`add_rel` with `BeforeLastCloseTrimmed`) on a text with many `<Relationships` before a `>` (or none) | a search for `>` from every occurrence: 3 MB ≈ 22 s (the TS regex backtracks the same way) | the `>` found is reused while it lies ahead; no `>` ends the search | `expand_self_closing_is_linear` |
| F6 | low (panic in debug) | lenient preset with `keep_comments`/`keep_pis` (not a TS preset): `<!-->`, `<?>` | the end search starts at `<` (TS `indexOf`), so `end - start` underflowed: panic with overflow checks, `Limit` error in release. Found by the `xml_lenient`, `raw_surgery` and `content_types` targets | saturating: an empty comment/instruction, as the TS `slice` | `lenient_empty_comment_and_pi_shapes` |
| F7 | high (process abort) | an inflate output the host cannot allocate: the S1 zeros, or any part above 1 GiB on wasm32 (a `Vec` cannot exceed `isize::MAX`, 2 GiB − 1, below the 2 GiB binary cap) | `handle_alloc_error` aborts the process (the .NET host included) and a capacity overflow panics (a trap of the wasm instance with `panic = "abort"`), where the TS throws a catchable `RangeError` | every growth of the inflate output goes through `try_reserve` (same amortized sizes); failure is `Limit{PartBytes}` | `impossible_inflate_allocation_is_an_error` (4 EiB declared: the test process aborted before) |

Timings are release builds on the development machine. A side effect of F7: the S1 zeros are now
written (`try_reserve_exact` + `resize`), so they are committed memory at once instead of lazily
zeroed `calloc` pages; allocated bytes are the same (the S1 tests of the memory record pass).

### Checked, no finding

- Panics: no `unwrap`/`expect`/unchecked indexing on document data (the few `[]` indexes are
  behind length checks or on ASCII positions found by search); string slicing only at positions of
  ASCII bytes or parser positions.
- Allocation from declared values: only the S1 path (now fallible). Everything else is bounded by
  the input: CFB streams ≤ the file (`read_chain` takes `min(size, chain, file)`), FAT ≤ one entry
  per file sector, zip names ≤ the central directory, entry count ≤ `len / 46` and
  `max_entries`, XML arenas ≈ 2.2 × the source, text decoding ≤ the input.
- Termination: inflate advances at least one bit per symbol and reads past the end as zeros only
  up to the `pos > tbts` check; FAT/miniFAT/DIFAT chains have cycle guards; the XML readers are
  iterative and every scan moves forward; the CFB storage walk recurses at most 32 levels.
- Limits: the part cap applies to stored and deflated entries (bytes produced, never declared);
  the total counts each entry once; entries over `max_entries` are refused; lenient depth counts
  open elements (`<?pi` elements of the docx/sheet presets included), strict depth counts like
  `XmlReader.Depth`; strict mode refuses DTDs, undeclared entities, unbound prefixes and duplicate
  expanded names.
- wasm32 truncation: every `u64 → usize` is `try_from` or bounded by the input length (zip ranges
  clamp to the archive, CFB offsets via `try_from`, inflate copies ≤ 261 bytes, the initial
  capacity ≤ 64 MiB).

### Proposals for gate 2 (behaviour changes, not applied)

| # | Evidence | Recommendation |
|---|---|---|
| S1 | measured by the memory verifier (above): a 132 B archive → 256 MiB of zeros, 340 B → 4 GiB held, +537 MB of permanent wasm memory. F7 removed the abort but not the amplification | an empty stream with a declared size > 0 is `Corrupt` (no real document has one) |
| P2 — XML cap by name | `Limits::part_cap` picks the cap by extension; Word opens a main part with any name (the content type decides), so a 2 GiB `word/document.bin` gets the binary cap and the XML reader keeps ≈ 2.2 × + the source (≈ 6.6 GiB) | the format crates read the parts they parse through one call that applies `max_xml_part_bytes` whatever the name (e.g. `Package::xml_part`), or the cap follows the content type |
| P3 — one resolution path | verified: with `CaseFold::Unicode` (the Sheet policy) the target `media/K.png` (U+212A KELVIN SIGN) resolves to `word/media/K.png`, which the package finds as `word/media/k.png`; the OPC rule (`fold_ascii`, the C#) finds nothing, so a validator using it does not check the part the library opens. Write (`CaseFold::None`, the clickCompliance case) is not affected | one call for format crates and hosts, `Package::resolve_target(source, target)` = resolver + the package's own lookup; and the Sheet policy moves to ASCII folding in stage 5 (G5) |
| P4 — wasm32 limits | on wasm32 a part cannot exceed 2 GiB − 1 and all memory (retained input included) is 4 GiB, so the default 2 GiB part / 4 GiB total are unreachable: allocation fails before the limit (now an error, F7) | `uo-wasm` sets its own `Limits` (e.g. 512 MiB per binary part, 1 GiB total) so the limit error comes first |
| P5 — names for hosts that extract | names are returned as written (TS parity): `../x`, `/abs`, backslashes, NUL, drive letters round trip unchanged (`zip_write` target). The resolver never returns empty, `.` or `..` segments, nor `\` or control characters from the target; the folder of the source part is taken as is | a `part_name` helper that says whether a name is safe as a relative path, for hosts that write parts to disk |

Observations (no change proposed now): a CFB directory of n entries pointing at the same chain
makes a reader of every stream produce n × the file size (each stream is bounded, the sum is not;
the format readers read named streams only; a total like the package's is a stage-5 option).
`Package::remove_part` is O(parts) per call (`retain` is the bulk path). `raw::remove_range` with
offsets that are not char boundaries returns the text without the side that does not slice (callers
pass parser positions).

### Fuzzing

Targets (`core/fuzz/src/lib.rs`, plain `fn(&[u8])`; `fuzz_targets/*.rs` wrap them for libFuzzer):
`zip_open` (open, every read within its cap and the total, lookups, `verify`, text), `zip_roundtrip`
(edit, save, reopen: same names in order and same bytes), `zip_write` (hostile names: any text,
`..`, `/`, `\`, NUL, longer than 65,535 bytes; read back against a model of `unzipSync`), `inflate`
(raw/declared/zlib/stream/capped, differential against miniz_oxide on the streams it accepts,
deflate and zlib round trips), `cfb`, `xml_lenient` (docx, sheet, slides and charts presets, tree
and positions walked), `xml_strict` (every prefixed name bound), `escape_roundtrip` (every escape
variant and `XmlWriter`, read back lenient and strict), `raw_surgery`, `rels` (write/read round
trip), `content_types`, `caseless` (against a model through edits), `part_text` (UTF-8/UTF-16
BOMs), `resolver` (clean names; `escape_segment` resolves back). Limits are small (1 MiB XML part,
2 MiB binary, 4 MiB total). Seeds: `--bin seed` builds `corpus/` (not versioned, about 4,000
files) from 60 small oracle .docx, the small Sheet/Slides samples, their parts and streams, 40
legacy .doc/.xls/.ppt (whole and cut), and generated quirks.

| Target | libFuzzer execs (30 min) | Corpus after | Stable driver execs (60 s) |
|---|---|---|---|
| zip_open | 457,148 | 1,280 | 647,242 |
| zip_roundtrip | 428,089 | 1,141 | 652,343 |
| zip_write | 630,417 | 2,714 | 430,831 |
| inflate | 2,937,307 | 5,421 | 1,838,177 |
| cfb | 9,122,658 | 811 | 12,663,742 |
| xml_lenient | 3,419,197 | 10,766 | 2,758,038 |
| xml_strict | 3,916,198 | 8,237 | 2,820,710 |
| escape_roundtrip | 2,731,286 | 3,396 | 720,549 |
| raw_surgery | 6,305,988 | 13,469 | 3,957,577 |
| rels | 1,398,901 | 6,237 | 598,184 |
| content_types | 1,304,054 | 4,918 | 699,250 |
| caseless | 1,490,051 | 2,321 | 625,941 |
| part_text | 42,267,785 | 1,728 | 4,848,572 |
| resolver | 31,427,378 | 3,605 | 15,615,830 |

libFuzzer: cargo-fuzz 0.13.2, nightly 1.98, WSL Ubuntu 24.04, AddressSanitizer, overflow checks
and debug assertions on (`[profile.release]` of the fuzz crate), `-timeout=10 -rss_limit_mb=2048`,
the 14 targets in parallel for 1,800 s each: 107.9 million executions, **0 crashes, 0 timeouts,
0 out-of-memory** (slowest unit < 1 s, peak RSS 443–668 MB). The stable driver (no coverage,
mutations seeded from the same corpus) found F6 in its first 20-second pass; the long runs above
were made after the fixes. On Windows, `cargo +nightly fuzz run` works with AddressSanitizer when
`clang_rt.asan_dynamic-x86_64.dll` (Visual Studio, `VC/Tools/MSVC/*/bin/Hostx64/x64`) is on
`PATH`; `-s none` does not link there (`__start___sancov_pcs`), and it runs 5–10 × slower than on
Linux.

CI: job `fuzz` of `.github/workflows/rust-core.yml` (Linux, nightly): builds the seeds and every
target, then 60 s per target (14 min); crashes are uploaded as the `fuzz-artifacts` artifact.

```
cd core/fuzz
cargo run --release --no-default-features --features fallback --bin seed
cargo +nightly fuzz run <target> -- -max_total_time=1800 -timeout=10
cargo run --release --no-default-features --features fallback --bin fallback -- --secs 60 [target...]
cargo run --release --no-default-features --features fallback --bin fallback -- --reproduce <target> <file>
```

## Gate 2 — summary and decisions (approved 2026-11-01)

Maintainer, verbatim: "Approve D1–D8" — all eight recommendations accepted.

Verified by the orchestrator after the three verifiers (not only reported): all 191 workspace
tests pass in release with all features; fmt and clippy clean; the parity capture regenerated from
the frozen TS is byte-identical and a single altered digest makes the replay fail; the 1,000-iteration
soak passes natively; the wasm tests pass in Node and WebKit; an 8 s fallback fuzz pass per target
with a new seed found nothing (the 30-min libFuzzer runs: 107.9 M execs, 0 crashes).

- **Parity**: 0 mismatches over the 2,742 oracle docx, the sheet/slides samples, .doc/.ppt and the
  quirk corpus; accepted only by name (G4 raw copy, G5 fixes).
- **Memory**: M1, M2 (native, wasm, WebKit, real Safari), M3 (budgets asserted +10%), M5 green;
  M4 not applicable yet.
- **Security**: F1–F7 fixed with regression tests, results identical to the TS.

Decisions (ids unique across the verifiers' reports; "Parity P1/P2" and "Security P2–P5" map below):

| id | Decision | Recommendation |
|---|---|---|
| D1 | S1: an empty deflate stream with a declared size > 0 is `Corrupt` (today: that many zeros, 132 B → 256 MiB, wasm +537 MB permanently) | approve |
| D2 | A cap on the XML tree per part (elements), since one hostile 256 MiB part can become several GB of tree (up to 17× the source) | approve, default ~10× the largest real part |
| D3 | The XML part cap applies to every part parsed as XML (security P2), not by extension | approve |
| D4 | Parity P1: keep entries that fflate drops after a `__proto__` entry | approve (keep Rust) |
| D5 | Parity P2: copy an entry raw on save only when its read matched the declared size/CRC; otherwise compress again | approve |
| D6 | Security P3: one `Package::resolve_target` used by format crates and hosts; Sheet to ASCII folding in stage 5 | approve |
| D7 | Security P4: wasm limits below the native defaults (512 MiB per part, 1 GiB total) | approve |
| D8 | Security P5: a "safe relative path" helper for hosts that extract to disk (addition only) | approve |

Not a gate-2 item: iOS Lockdown Mode disables WebAssembly (product decision before 0.10 ships).

## Gate 2 decisions — implemented (2026-11-01)

All eight decisions are in `src/`, each with tests that failed before the change (an assertion of
the old behaviour or an API that did not exist). Parity differences they cause are accepted by
name in the tests, never by loosening a comparison.

### D1 — empty deflate stream with a declared size is `Corrupt`

- `flate::inflate_raw` (and so `inflate_zlib`, `check_raw` and every package read): an empty
  stream with `declared > 0` is `Error::Corrupt("empty deflate stream with a declared size")`,
  before any allocation; no declared size or a declared 0 stay the empty output.
- Measured: the 132 B docx declaring a 256 MiB `document.xml` allocates 12 B (was 256 MiB); the
  340 B archive with three 2 GiB parts allocates 5 B per read and holds nothing (was 4 GiB; the
  test is no longer `#[ignore]`d); `verify()` of two parts declaring 64 MiB allocates 5 B; a save
  fails the same way. In wasm the S1 round trip leaves `memory.buffer.byteLength` at 2,621,440 B
  (was +537,001,984 B, permanent); the test now asserts no growth.
- Tests: `flate_fflate` `empty_input_with_a_declared_size_is_corrupt_d1` (was
  `empty_input_returns_the_declared_buffer`), `s1_empty_deflate` (both tests assert `Corrupt` and
  < 4 KiB), `uo-wasm/tests/opc_memory.test.mjs` (S1), `security_regressions`
  `impossible_inflate_allocation_is_an_error` (its S1 input is now `Corrupt`; the `try_reserve`
  growth of F7 stays, reachable only by an output above the host's memory).
- Parity, accepted by name D1: `flate_differential` 16 of the 2,000 mutated streams (empty after
  truncation, with a declared size); `zip_fflate` `lying_sizes`; `opc_parity` 6 packages
  (`quirk/006-size-lies` and `quirk/025-bad-offset`, three apps each: the bad offset puts the data
  range past the archive, so the stream is empty). The replay accepts only when replacing exactly
  the D1 entries by fflate's zeros gives the captured package
  (`parity_support::load_as_fflate_d1`), and compares every other facet on that package.

### D2 — elements per parsed part

- `LimitKind::XmlElements`; `ParseOptions::max_elements: Option<u32>` (`None` in `LENIENT` and
  `STRICT`, the TS); `Limits::max_xml_elements`. The check is one comparison in `push_element`
  before an element is created; it changes no tree and allocates nothing (asserted: same dump and
  same allocated bytes with the cap at the element count, error one below).
- Default **20,000,000**, about 10 × the largest real part. Measured over the 2,819 git-tracked
  OOXML packages: `sheet/samples/giant-1m.xlsx` `xl/worksheets/sheet1.xml` **2,010,005**
  elements (31.7 MB, the largest); `giant-200k.xlsx` 430,005; the largest oracle `document.xml`
  (`justify4/sw-16-8.docx`, largest by bytes too, 188,270 B) **6,657**. At the cap, the minimal
  hostile shape (`<a/>`, 7 × its source, M5) is about 560 MB of tree instead of ~1.9 GB for a
  256 MiB part.
- Tests: `gate2_decisions` `d2_element_cap_is_an_error_and_changes_no_tree` (docx positions,
  sheet, strict, a 100,000-element flat part); `package_budgets` asserts the default ≥ 9 × the
  giant sheet; D3 below applies it through the package.

### D3 — one entry point for parsing a part

- `Package::xml_part(name, &ParseOptions) -> Result<xml::Document>`: found like `find` (exact,
  then the `CaseFold` policy); read with `max_xml_part_bytes` whatever the name, also for bytes
  already read through `part` with a binary cap or set by the caller; decoded like `part_text`;
  parsed with the options' depth and element limits lowered to `Limits::max_xml_depth` and
  `max_xml_elements`; limit errors name the part; a missing part is `MissingPart`. The package
  module documents that format crates parse parts only through it; `Limits::part_cap(name)` stays
  for raw reads. `rels::from_document` and `ContentTypes::from_document` read relationships and
  content types from its result (`parse_rels`/`ContentTypes::parse` delegate to them).
- Behaviour to know: the TS sheet and slides presets have no depth limit; through `xml_part` they
  get `max_xml_depth` (1,000). No corpus document is that deep; the parity replay still parses
  with the presets directly.
- The M2 soak (native and wasm) and the M3 scenarios now parse through `xml_part`
  (`memory_support::read_and_parse_all`, `uo-wasm/src/opc_memtest.rs`): numbers unchanged (giant
  open + read + parse 355,417,287 B allocated, was 355,417,360 B).
- Tests: `gate2_decisions` `d3_xml_part_applies_the_xml_cap_whatever_the_name` (a 4.4 KB
  `word/document.bin` under a 1,000 B XML cap: refused with a peak < 64 KiB, also after a raw
  read), `d3_xml_part_applies_the_package_depth_and_elements`,
  `d3_xml_part_finds_and_decodes_like_the_text_readers`,
  `d3_rels_and_content_types_from_the_parsed_part` (with `rels::source_part_of`).

### D4 — keep the entries fflate drops after `__proto__`

No code change (uo-opc already keeps them): the replay's `PENDING` acceptance is now named
`D4` (3 packages, `quirk/005-proto-then-numeric-names` in the three apps).

### D5 — raw copy on save only when verified

- `flate::check_raw(data, declared, cap) -> Result<StreamCheck { len, stream_len, crc32 }>`: the
  same inflate (`inflt` is generic over its store: the `Vec<u8>` store has the same growth as
  before, so every M3 number of a read is unchanged; the `Window` store is a 64 KiB ring, above
  fflate's longest distance of 32,769, whose bytes enter the CRC every 32 KiB), the same results
  and errors, no output kept.
- `Package` keeps per entry what is known of its stream (`len`, `stream_len` = fflate's `bt`,
  CRC-32): a read records the first two, the save or `verify()` the CRC (from the kept output if
  there is one, else through `check_raw`). An entry is copied raw only when `len` and
  `stream_len` equal the declared size, the CRC is the declared one, and the declared compressed
  range lies inside the archive (the local header only gives the data start, through its name and
  extra lengths: data found at a wrong place fails the size or CRC check). Otherwise the save
  writes the bytes the reader produces: deflated at the save level, stored if the source was
  stored (with the real CRC); `stored` names as before. An entry that cannot be read fails the
  save with the read's error (before: copied raw as is) — the TS cannot open such a file at all.
- Verification counts toward `max_total_bytes` like a read (each entry once): it keeps no output,
  but the total bounds the work of archives whose entries overlap one stream; `produced_bytes`
  after a save includes the entries it verified.
- Numbers (`package_budgets`, re-recorded): deck (17.2 MB; 64 MiB deflated zeros, 16 MiB stored)
  edit + save: allocated 54,421,635 B (was 54,282,370: the windows and Huffman tables), peak
  34,478,909 B (the output's capacity, unchanged); the 64 MiB part is never held (peak < 64 MiB
  and < 2 × output + 1 MiB, asserted), and the save copies it raw (same compressed bytes,
  asserted). Time (release): first save 204–215 ms (verifying 64 MiB inflated + 80 MiB of CRC),
  a second save 7 ms (verification kept, allocations without the windows). Giant save untouched
  (every part read before): CRC only, allocations unchanged (8,549,574 B), 53–75 ms against
  1.0–1.6 ms with the verification kept. Giant save with the sheet edited: 25,020,549 B (+1.3%,
  the never-read entries verified); sw-16-8 edit + save 4,493,829 B (+4.7%); deck open + parse
  1,188,339 B / peak 147,208 B (+1 KB: the per-entry record and the second index cell).
- Memory gate re-run: M2 native 100 iterations 771,231 B flat, 1,000 iterations 771,375 B × 21
  flat (20.3 s release); wasm 1,000 iterations 0 B live, `byteLength` 2,621,440 B flat (23 s,
  was 16–18 s: the CRC of the save and of `verify`). M1: `gate2_decisions`
  `new_entry_points_do_not_leak`, `flate_memory` (`check_raw`).
- The deck fixture (`memory_support::deck_with_large_media`) declared CRC 0 for its zeros; it
  now declares the real one (else D5 rewrites it, as it should).
- Cross readers (`UO_TS_PARITY=1 UO_DOTNET_PARITY=1 … save_cross_readers`): 10,053 saved
  archives (3,351 packages, the D1 ones saved as the replay compares them) read identically by
  fflate, uo-opc and .NET `ZipArchive`: the **9 .NET differences (quirk/006, 025, 038) are
  gone**. The test's "pending" category is removed: any .NET difference now fails it. G4 in the
  replay is unchanged (324 raw saves keep a stored source entry stored: 10 other fixtures, 52
  slides, 262 quirks); the parity save facets do not change, since a rewritten entry holds the
  bytes the reader produced.
- Tests: `gate2_decisions` `d5_verified_entries_are_copied_raw_and_lying_ones_written_again`
  (good, CRC lie, truncated by the declared size, shorter than declared, stored CRC and size lies,
  range past the archive; levels 6 and 0; every saved entry inflates to its declared size and
  CRC), `d5_an_entry_that_cannot_be_read_fails_the_save`,
  `d5_never_read_entries_are_verified_once_without_their_output`; `flate_fflate`
  `check_raw_matches_inflate_raw_past_the_window` (3 MiB outputs at levels 0, 1, 6, 9, stored
  blocks of 65,535 B, distance 32,769); `flate_differential` runs `check_raw` on all 4,000
  inflations; `package_budgets` `m3_deck_with_large_media`.

### D6 — one resolution path

- `Package::resolve_target(source_part, target) -> Option<&str>`:
  `part_name::resolve_relationship_target`, then the exact name, else the first part (in order)
  equal under ASCII folding, whatever the `CaseFold` policy; returns the stored name `part` and
  `xml_part` open (borrowed from the package instead of a `String`: no allocation per lookup).
  The ASCII index is built on the first fallback and dropped on any change of the part set, and
  is the caseless index itself under `CaseFold::Ascii`. For hosts: `rels::source_part_of` (the
  part a `.rels` describes) with `xml_part` + `rels::from_document` + `resolve_target` is
  everything a validator needs. The apps' legacy target rules stay as parity policies in
  `parity_support` until stage 5.
- Tests: `gate2_decisions` `d6_resolve_target_returns_the_name_the_package_opens` (the three
  policies; the U+212A KELVIN SIGN target of security P3 resolves to nothing while Sheet's
  Unicode `find` would open `k.png`; the index follows `set_part`, `remove_part`, `retain`);
  `cache_caps` `m5_ascii_index_of_resolve_target` (4,000 parts: 365,263 B; 100,000 missing
  lookups do not grow it; dropped on change; one index under `CaseFold::Ascii`).

### D7 — wasm32 limits

- `Limits::NATIVE` (2 GiB per binary part, 4 GiB total) and `Limits::WASM32` (512 MiB per binary
  part, 1 GiB total, XML part min(256 MiB, 512 MiB) = 256 MiB); `Limits::DEFAULT` is one or the
  other by `#[cfg(target_arch = "wasm32")]` on the constant.
- Tests: `limits::tests::native_and_wasm32_defaults`; the `uo-wasm` export
  `uo_opc_memtest_default_limit(field)` (feature `opc-memtest`, test-only) and the node test
  "D7", which read `Limits::DEFAULT` as compiled for wasm32.

### D8 — safe relative paths

- `part_name::safe_relative_path(name) -> Option<String>`: refuses empty names, a leading `/`,
  `\`, `:` (drive letters and alternate data streams), `< > " | ? *`, NUL and every control
  character, empty segments (a trailing `/` included), `.` and `..`, segments ending in `.` or a
  space, and the Windows device names (`CON`, `PRN`, `AUX`, `NUL`, `COM0`–`COM9`, `LPT0`–`LPT9`,
  with ¹ ² ³, `CONIN$`, `CONOUT$`, `CLOCK$`, any case, any extension); returns the name.
  Addition only.
- Tests: `part_name::tests::safe_relative_paths` (15 safe, 47 hostile names);
  `gate2_decisions` `d8_safe_relative_path_over_the_quirk_corpus` (151 distinct names, 120 safe,
  31 refused: `./…`, `/abs.xml`, `a/../b.xml`, backslash names, trailing `/`).

### Fuzzing

`FUZZ_LIMITS.max_xml_elements` 20,000. `zip_open` parses every part through `xml_part`
(within the XML cap, depth and element limits, errors naming the part), resolves names and
checks `safe_relative_path`; `zip_roundtrip` accepts a failed save only when a part failed to
read, and checks that every entry of a save inflates exactly to its declared size and CRC;
`inflate` compares `check_raw` with `inflate_raw`; `caseless` checks `resolve_target` against a
model; `resolver` checks the safe-path invariants. Fallback driver, 10 s per target: 0 crashes
(zip_open 38,167 execs, zip_roundtrip 109,278, zip_write 70,686, inflate 251,690, cfb
2,181,005, xml_lenient 455,899, xml_strict 513,431, escape_roundtrip 159,388, raw_surgery
815,734, rels 119,469, content_types 127,910, caseless 57,092, part_text 817,037, resolver
2,088,131). The 30-minute libFuzzer runs were not repeated.

### Checks run

`cargo fmt --all --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`
(and on `core/fuzz`); `cargo test --workspace --release --all-features`: 207 tests pass (191
before); the debug `cargo test -p uo-opc` takes 74 s; the parity replay: 0 mismatches; the wasm
build (`count,opc-memtest`) with `boundary.test.mjs` and `opc_memory.test.mjs`; the 1,000-iteration
native soak. Not run here: the WebKit runner (`playwright` is not installed in this worktree) and
real Safari.

Files: `src/{error,flate,limits,package,part_name,rels,content_types,zip}.rs`,
`src/xml/read.rs`; tests `gate2_decisions.rs` (new), `flate_fflate.rs`, `flate_differential.rs`,
`flate_memory.rs`, `s1_empty_deflate.rs`, `security_regressions.rs`, `zip_fflate.rs`,
`cache_caps.rs`, `package_budgets.rs`, `opc_parity.rs`, `parity_support/mod.rs`,
`memory_support/mod.rs`; `uo-wasm/src/opc_memtest.rs`, `uo-wasm/tests/opc_memory.test.mjs`;
`fuzz/src/lib.rs`. Line endings: `flate_fflate.rs`, `flate_memory.rs` and `part_name.rs` were
committed with CRLF and are LF now (five other files of the crate still are CRLF:
`tests/cfb_memory.rs`, `tests/cfb_support/mod.rs`, `tests/zip_memory.rs`,
`tests/xml_support/gen_strict_inputs.mjs`, `tests/xml_support/strict_oracle.cs`).

```
cargo test -p uo-opc --release --test gate2_decisions -- --nocapture
cargo test -p uo-opc --release --test s1_empty_deflate -- --nocapture   # no longer ignored
UO_M3_RECORD=1 cargo test -p uo-opc --release --test package_budgets -- --nocapture --test-threads 1
UO_TS_PARITY=1 UO_DOTNET_PARITY=1 UO_TS_ROOT=<checkout of main> cargo test -p uo-opc --release --test opc_parity save_cross_readers -- --nocapture
```

### D5 follow-up: save verification excludes overlaps (approved 2026-11-01)

Maintainer, verbatim: "Exclude overlaps". The first D5 implementation counted the verification of
never-read entries toward `max_total_bytes`, so a package whose untouched media exceed the total
(1 GiB on wasm32) could not be saved. Now:

- the verification holds no memory (64 KiB window) and is **not** counted toward the total;
- an entry whose compressed range overlaps another entry's (computed once at open, one sweep over
  the sorted ranges) is never verified nor copied raw: the save reads it under the normal limits,
  so a crafted archive pointing many entries at one stream is bounded by the total;
- every other entry is checked at most to `MAX_DEFLATE_RATIO` (1,100) times its compressed bytes
  (1 MiB at least), so a save inflates at most that ratio times the archive; a stream past it is
  not intact and the read that follows reports the limit.

Tests: `gate2_decisions::d5_intact_entries_save_under_any_limits_without_counting` (16 MiB of
untouched media saved raw under a 1 MiB part cap and 2 MiB total, `produced_bytes` 0) and
`d5_overlapping_entries_are_read_under_the_limits_never_verified` (201 entries on one 1 MiB stream:
`Limit{TotalBytes}` under 16 MiB, written again under 512 MiB). The deck budget asserts the
media are no longer counted. 209 workspace tests, wasm/WebKit, the .NET cross-read (10,053
archives) and the fuzz fallback pass.

## Process metrics (baseline for the model comparison)

Every role of uo-opc ran on **Opus** (the "Models" rule of the skill came after it), so this is
the reference for the Sonnet ports. Times are the wall time of each agent; stages 3 and 4 ran
their agents in parallel.

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| 1 | planner (Opus) | 6.7 min | 24 |
| 1 | reuse specialist (Opus) | 4.8 min | 44 |
| 2 | analysis skeptic (Opus) | 10.2 min | 49 |
| 3 | port: flate, zip, lazy package (Opus) | 20.3 min + 3.0 min follow-up | 55 + 17 |
| 3 | port: CFB (Opus) | 10.4 min | 30 |
| 3 | port: XML, rels, content types (Opus) | 29.6 min | 99 |
| 3 | part names and limits (orchestrator) | — | — |
| 4 | parity verifier (Opus) | 54.5 min | 122 |
| 4 | memory verifier (Opus) | 35.3 min | 107 |
| 4 | security specialist (Opus) | 85.5 min | 118 |
| gate 2 | D1–D8 implementation (Opus) | 40.1 min | 152 |

Wall time per stage: plan + skeptic ≈ 17 min; port ≈ 30 min (+ 3 min follow-up); verification
≈ 86 min; gate-2 decisions ≈ 40 min, plus the D5 follow-up by the orchestrator.

**Parity rounds** (the parity verifier sending a port bug back): **0** — no port bug found in the
corpus or the quirk corpus; every difference was an approved decision (G4, G5) or a new one
(D4, D5).

**Rework** (what had to be redone after an agent reported done):

| # | What | Why | Fixed by |
|---|---|---|---|
| R1 | The fflate expectations of 4 test files had no generator in the repo | not reproducible; one generator was not deterministic (zip mtime) | port agent follow-up (3 min) |
| R2 | Error for zip names over 65,535 bytes was `Corrupt` | wrong error class | orchestrator |
| R3 | `Package` re-export missing at the crate root | stubs compiled in parallel | orchestrator |
| R4 | Files committed with CRLF (stage 3 and security edits) | Windows editing, no `.gitattributes` | orchestrator (`core/.gitattributes`) |
| R5 | Security F1–F7 (quadratic inflate tables inherited from fflate, quadratic rels/raw edits, a span underflow found by fuzzing, abort on an impossible allocation) | found in stage 4, each with a regression test | security specialist |
| R6 | D5 counted the save verification toward the memory total | design consequence not raised before the change | orchestrator ("Exclude overlaps") |

Not port rework: the `uo-alloc` self-tests failed in release (allocation elision); fixed in the
harness with `black_box`.

Stages 5–6 (Opus, as the rest of uo-opc):

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| 5 | simplification and algorithms (Opus) | 45.9 min | 158 |
| 6 | final skeptic (Opus) | 12.8 min | 73 |
| 6 | stage-6 findings M1, S1–S8 (Opus) | 28.0 min | 112 |
| 5 (uo-jsnum D2) | simplification and algorithms (Opus 5.5): the JS helpers to uo-jsnum, in one session with uo-jsnum D2 and uo-fonts D5 | ≈ 80 min for the three | ≈ 120 for the three |

| # | What | Why | Fixed by |
|---|---|---|---|
| R7 | Arena reservation wrapped on wasm32 (M1): a 256 MiB part grew wasm memory by 2.14 GB | stage-5 size estimate computed in `usize` | stage-6 fix (u64 saturating) |
| R8 | Edit helpers parsed without the D2/D3 limits (S1) | helpers predate D3 | stage-6 fix (`Limits::bounded`) |
| R9 | A contaminated benchmark run (three agents on the machine) | benchmark started while agents ran | orchestrator: re-run idle |

## Stage 5 — evolution (simplification and algorithms, 2026-11-01)

Result-preserving changes applied one at a time, each measured before and after, with the parity
replay (0 mismatches against the committed capture, unchanged), every test, the memory gate and
the fuzz fallback as the safety net. Result-changing proposals are listed at the end with their
evidence; none is applied.

### Benchmark (`tools/bench/run.mjs --suites opc`)

- `tools/bench/opc/ts.mjs`: the frozen 0.9.x packages and parsers (each app's own code, as the
  parity capture), one node process per scenario; time = best run; memory = growth of the
  maximum RSS on the first run and heapUsed + arrayBuffers kept after GC with the outputs alive
  (the memory verifier's method).
- `core/crates/uo-opc/examples/opc_bench.rs` (native, release) and the `uo-wasm` feature
  `opc-bench` (`src/opc_bench.rs`, bench-only exports, never shipped) run with
  `tools/bench/opc/wasm.mjs`; both run the same scenarios, `tests/bench_support/mod.rs`, and
  count memory with `uo-alloc` (allocated bytes; peak live bytes above the start; bytes kept with
  the outputs alive); wasm also reports the growth of `memory.buffer.byteLength` (a permanent
  high-water mark).
- Scenarios: (a) the 2,742 oracle docx, each opened, every XML-like part parsed with Write's
  preset (positions on) through `xml_part`, every other part read (memory per document); (b) the
  same on `giant-1m.xlsx` with the Sheet presets, the package kept; (c) `xl/worksheets/sheet1.xml`
  of the giant inflated with its declared size (31.7 MB); (d) the edit of `ts_budgets.mjs` (an
  element before the last end tag of the main part, a new part) and the save, on
  `sw-16-8.docx` and on the giant (the 31.7 MB sheet deflated again). Every runner reports the
  same parts, elements and bytes as the TS (checked in the tables).
- Machine: Intel i5-14600KF, Windows 11, node 24.15, rustc 1.97.1; the native runner carries the
  counting allocator. Results: `tools/bench/results/2026-11-01-9ac45738-dirty-baseline.md`
  (the source of 9ac45738 with the bench tooling added) and
  `2026-11-01-122ead8e-dirty-stage5.md` (with the changes below).

### Baseline (before any change)

| Scenario | | TS (V8) | Rust native | Rust wasm |
|---|---|---|---|---|
| (a) oracle, 2,742 docx | best | 584.5 ms | 259.6 ms | 279.6 ms |
| | allocated / peak per doc | — / 29.3 MB RSS | 394.2 MB / 0.83 MB | 361.3 MB / 0.82 MB |
| (b) giant open + read + parse | best | 742.4 ms | 165.2 ms | 185.4 ms |
| | allocated / peak | — / 826.6 MB RSS | 355.4 MB / 146.1 MB | 355.4 MB / 146.1 MB |
| | kept | 31.9 MB | 34.6 MB | 34.6 MB |
| | wasm memory growth | — | — | 203.9 MB |
| (c) inflate 31.7 MB | best | 60 ms (529 MB/s) | 38.5 ms (824 MB/s) | 42.7 ms (743 MB/s) |
| | allocated / peak | — | 92.8 MB / 34.2 MB | 92.8 MB / 34.2 MB |
| (d) sw-16-8 edit + save | best | 2.5 ms | 0.8 ms | 1.2 ms |
| | allocated / peak | — / 5.0 MB RSS | 2.06 MB / 0.73 MB | 2.19 MB / 0.79 MB |
| (d) giant edit + save | best | 575.5 ms | 818.5 ms | 1,007.9 ms |
| | allocated / peak | — / 89.4 MB RSS | 181.5 MB / 95.2 MB | 181.7 MB / 95.2 MB |
| wasm module | `opc-memtest` / `count,opc-memtest` / `opc-bench` | | | 252,423 / 252,914 / 270,853 B |

### Changes applied (result-preserving)

Numbers are native (`opc_bench`, allocation counts are deterministic) unless marked; M3 numbers
are `tests/package_budgets.rs`.

| # | Change | Before → after |
|---|---|---|
| C1 | **Inflate output growth bounded by the declared size** (`flate::try_grow`): it still starts at 3 × the input and doubles (never preallocated from a declared size), but a step never goes past min(declared, cap), so an output that fills its declared size ends exact, without the 2 × overshoot and the `shrink_to_fit` copy | (c) allocated 92.8 → 58.7 MB, peak 34.2 → 31.7 MB; M3 deck 64 MiB read 228.2 → 147.0 MB allocated, peak 81.2 → 67.1 MB; (a) 394.2 → 356.6 MB; (b) 355.4 → 321.3 MB |
| C2 | **Inflate reads 4 bytes per bit fetch** (`flate::word`: one bounds check instead of 2–3 bytes with a `u64 → usize` conversion each; every mask is ≤ 15 bits, so the values are fflate's) and **back-reference copies by doubling slices** (`extend_from_within` of everything written since the start of the copy, the output being periodic there) instead of one byte at a time | (c) 36.9 → 27.9 ms (824 → 1,136 MB/s; wasm 743 → 1,133 MB/s at the end); (a) 263.9 → 238.9 ms; (b) 166.9 → 156.8 ms |
| C3 | **`Package::xml_part` hands the inflated text to the tree**: a part inflated by the call is decoded in place (UTF-8 keeps the buffer, the BOM removed with `drain`) and becomes the tree's source; the package keeps nothing (a later read inflates again, not counted again toward the total). `xml::Source` replaces `Arc<str>` as the tree's source: `Arc<String>` or `Arc<str>`, cloned by reference count, `Deref<Target = str>` (`parse` takes `impl Into<Source>`: `String`, `Arc<str>` without a copy, `&str`/`Cow` with one) | (b) allocated 321.3 → 289.5 MB, peak 146.1 → 114.4 MB, kept 34.6 → 2.85 MB (the input copy only); (a) 356.6 → 324.9 MB, peak 0.83 → 0.63 MB, 238.9 → 222.8 ms |
| C4 | **Huffman tables reused across blocks** (`h_map` writes into the stream's buffers; counts and first codes on the stack; code lengths in a 320-byte array) | (a) 324.9 → 318.3 MB; (c) 58.66 → 57.39 MB (peak +26 KB: the tables live through the stream) |
| C5 | **CRC-32 by 8 bytes** (slicing by 8; the byte table is fflate's, the 7 others derived at compile time; test against the byte loop and the CRC-32 check value) | M3 deck first save (verifies 64 MiB + 16 MiB) 197 → 103 ms; giant save untouched (verifies the sheet) 71 → 37 ms; wasm +7,168 B (tables) |
| C6 | **One compressor per writer, output streamed, exact archive**: `flate::Deflater` keeps miniz_oxide's state (allocated once, `reset` per entry) and appends to the caller's buffer through `compress_to_output` (no `input.len() / 2` zeroed buffer); the bytes are those of `deflate_raw` (test: 9 levels × 11 inputs through one compressor). `ZipWriter::add` deflates straight into the archive and patches the size. `Package::save` decides every entry first (raw, stored, or a range of one deflate buffer), computes the length with `zip::archive_len` and writes into `ZipWriter::with_capacity(len)`: capacity = length | M3 deck edit + save 54.4 → 17.8 MB allocated, peak 34.5 → 17.25 MB, capacity 34.5 MB → exact; giant save untouched 8.55 → 3.30 MB, peak 5.70 → 2.85 MB; giant save with the sheet deflated 25.0 → 11.0 MB, peak 18.7 → 6.6 MB; `ZipWriter` with 100 small entries 25.4 → 0.37 MB; (d) docx 1.79 → 1.50 MB, 0.8 → 0.5 ms; (d) giant 146.1 → 131.8 MB, 807 → 772 ms |
| C7 | **XML arenas reserved from a byte count** (`read::Estimate`: start tags = `<` not followed by `/`, children = those + text runs (`>` not followed by `<`), attributes = `=`; counted 8 bytes at a time with SWAR, also without SIMD in wasm). Reserved only when the whole reservation fits in 3 × the source (`RESERVE_PER_SOURCE_BYTE`: real trees are 1.4–2.2 ×) and within `max_elements`; otherwise the arenas double as before. A real part is built without the doubling chain; inflated counts (markup in comments, values, a root that ends early) reserve at most what a real part of that size holds, below the M5 hostile shapes (7–17 ×). Same tree (`finish` still shrinks to fit) | (b) allocated 288.3 → 197.2 MB, peak 114.4 → 103.1 MB, **wasm memory growth 197.0 → 116.7 MB**; M3 sheet parse alone 259.7 → 168.7 MB, peak 111.5 → 100.2 MB; (a) 318.3 → 253.0 MB, peak 0.63 → 0.54 MB, wasm growth 0.85 → 0.59 MB; time: native (b) 148 → 151 ms, (a) 225 → 231 ms; wasm (b) 158 → 173 ms, (a) 242 → 252 ms (the count adds 15 ms on the giant in wasm with SWAR; a byte loop added 32 ms) |
| C8 | Simplifications: `limits::is_xml_like` compares without lowercasing a copy (it runs on every read through `part_cap`); one JS array-index rule (`zip::array_index`) for zip names and XML attribute order instead of two; the canonical renaming reads attribute names as source spans instead of copying each one | (a) 253.03 → 252.80 MB; M3 stored-part read 9 B → 0 B |

Wasm size: `opc-memtest` 252,423 → 266,269 B (+13,846: the CRC tables 7,168, the rest the
streaming compressor, the planned save, `Source` and the estimate); `count,opc-memtest`
252,914 → 266,760 B; `opc-bench` 270,853 → 284,701 B. Safari 15 baseline kept (no SIMD).

API changes (crate not published yet): `xml::Document::source()` returns `&xml::Source`;
`xml::parse` takes `impl Into<Source>`; added `xml::Source`, `flate::Deflater`,
`zip::archive_len`, `ZipWriter::with_capacity`; `Package::xml_part` no longer caches the bytes
it inflates.

### After (all changes)

| Scenario | | TS (V8) | Rust native | Rust wasm |
|---|---|---|---|---|
| (a) oracle, 2,742 docx | best | 559.4 ms | 226.9 ms | 244.6 ms |
| | allocated / peak per doc | — / 28.7 MB RSS | 252.8 MB / 0.54 MB | 219.7 MB / 0.53 MB |
| (b) giant open + read + parse | best | 699.1 ms | 149.6 ms | 172.9 ms |
| | allocated / peak | — / 834.9 MB RSS | 197.2 MB / 103.1 MB | 197.2 MB / 103.1 MB |
| | kept | 31.9 MB | 2.85 MB | 2.85 MB |
| | wasm memory growth | — | — | 116.7 MB |
| (c) inflate 31.7 MB | best | 59.4 ms (534 MB/s) | 27.6 ms (1,149 MB/s) | 28.0 ms (1,133 MB/s) |
| | allocated / peak | — | 57.4 MB / 31.8 MB | 57.4 MB / 31.8 MB |
| (d) sw-16-8 edit + save | best | 2.4 ms | 0.6 ms | 0.9 ms |
| | allocated / peak | — / 5.2 MB RSS | 1.50 MB / 0.70 MB | 1.56 MB / 0.77 MB |
| (d) giant edit + save | best | 554.4 ms | 792.3 ms | 957.1 ms |
| | allocated / peak | — / 88.3 MB RSS | 131.8 MB / 95.2 MB | 131.9 MB / 95.2 MB |

The giant save is the one scenario where the TS is faster: miniz_oxide at level 6 spends 735 ms
on the 31.7 MB sheet against fflate's 669 ms (proposal R4). Its 95 MB peak is the bench's edit
(the sheet read through `part_text`, cached by the package, copied and concatenated: 3 × 31.7 MB),
not the save (M3: 6.6 MB).

### Memory gate re-run

- M1: every `assert_no_leak` test passes (`api_memory`, `package_memory`, `zip_memory`,
  `flate_memory`, `xml_memory`, `gate2_decisions`, …).
- M2: native 100 iterations 771,231 B flat; 1,000 iterations 771,375 B × 21 flat (12.6 s
  release, was 20.3 s); wasm 1,000 iterations 0 B live, `byteLength` **2,293,760 B** high-water
  (was 2,621,440 B), 13.4 s in Node (was 23 s), 15.2 s in WebKit 26.5.
- M3 (`package_budgets.rs`, every budget lowered to the new number; capacity = length asserted
  for the saves):

| Operation | Allocated before → after | Peak before → after |
|---|---|---|
| giant open + read all + parse all | 355,418,000 → 197,243,000 | 146,113,000 → 103,061,000 |
| giant kept after open + read + parse | 34,571,000 → 2,850,000 | — |
| giant save untouched | 8,550,000 → 3,298,000 | 5,697,000 → 2,850,000 |
| giant parse `sheet1.xml` alone | 259,726,000 → 168,703,000 | 111,543,000 → 100,211,000 |
| giant save with the sheet deflated | 25,021,000 → 11,017,000 | 18,717,000 → 6,627,000 |
| sw-16-8 open + read + parse `document.xml` | 2,311,000 → 1,331,000 | 822,000 → 724,000 |
| sw-16-8 edit + save | 4,494,000 → 2,660,000 | 897,000 (unchanged: the edit's parse) |
| deck `open_owned` + parse all XML | 1,189,000 → 848,000 | 148,000 → 146,000 |
| deck edit + save | 54,422,000 → 17,769,000 | 34,479,000 → 17,255,000 |
| deck read the 64 MiB deflated part | 228,154,000 → 146,998,000 | 81,157,000 → 67,110,000 |
| `deflate_raw` of 4 bytes (one-shot API, unchanged) | 253,624 | 253,622 |
| `ZipWriter`, 1 deflated entry / 100 entries (new) | 319,774 / 367,222 | — |

- M5: unchanged (`cache_caps`; the tree factors 3.77 / 8.56 / 17.13 / 7.00 × and 40.4
  B/element for the 100k-cell sheet are the same: the estimate never changes what is kept).
- Fuzz fallback, 10 s per target: 0 crashes (zip_open 87,625 execs, zip_roundtrip 223,779,
  zip_write 149,992, inflate 499,709, cfb 4,376,792, xml_lenient 725,167, xml_strict 824,804,
  escape_roundtrip 277,876, raw_surgery 1,288,813, rels 227,271, content_types 214,555,
  caseless 107,029, part_text 1,577,903, resolver 3,596,898). Cross readers
  (`UO_TS_PARITY=1 UO_DOTNET_PARITY=1`): 10,053 saved archives read identically by fflate, uo-opc
  and .NET `ZipArchive`.

### Considered, not applied

- `Package::remove_part` is O(parts) (a `Vec::remove` of ~170-byte parts and an index scan):
  removing half of 10,000 parts one by one takes 115 ms, of 50,000 parts 7.4 s, where `retain`
  takes 0.6 ms and 3.5 ms. Tombstones would make it O(1) but touch every index of the package;
  the format crates remove a handful of parts per edit and `retain` is the bulk path. Left as is.
- Inflate's initial capacity stays fflate's 3 × the input (a larger start bounded by the input
  would save one growth step on XML that compresses more than 3 ×; small gain, not measured as
  worth a departure).
- `flate::deflate_raw` (one-shot, used by `zlib`) keeps `compress_to_vec`.

### Result-changing proposals (not applied; maintainer decides each)

Evidence: `cargo run --release -p uo-opc --example opc_rule_evidence [-- --all]` runs each app's
own rule and lookup (the 0.9.x TS, as reproduced by `tests/parity_support` with 0 parity
mismatches) against the D6 path (`Package::resolve_target`: the OPC rule of the C# `OpcPartName`
plus ASCII case folding) over the parity corpus: 2,819 git-tracked OOXML packages (9,289
relationships the apps keep, 19,745 XML parts) and the quirk corpus with the three apps (526
packages, 22,704 relationships, 8,074 XML parts).

| # | Proposal | Evidence | Recommendation |
|---|---|---|---|
| R1 | **Write's target rule** (`./` stripped, `/x` → `x`, else `word/${target}` without resolving `..`) → the D6 path | Real files: one document, two relationships: `site/assets/samples/demo.docx` (written by Microsoft Office Word, `docProps/app.xml`), `word/_rels/document.xml.rels` targets `../customXml/item1.xml` and `../customXml/item2.xml`: Write resolves `word/../customXml/item1.xml`, missing; D6 finds `customXml/item1.xml` and `item2.xml` (cover-page properties Word wrote there; OPC Part 2 resolves a relative target against the source part, RFC 3986). Write does not render custom XML parts (the reader only unwraps `w:customXml` elements), so no layout changes. No real relationship goes from found to missing. Quirks (Write): missing → found by parent segments 334, case 176 (OPC compares part names ASCII case-insensitively), fragments 168, percent-encoding 163; 1 found in another part (below) | switch to D6 |
| R2 | **Sheet's lookup with Unicode lowercase** → ASCII folding (the OPC rule) | 0 lookups differ in the real files and in the quirk corpus; only the security P3 shape differs (U+212A KELVIN SIGN folds to `k` in Unicode, so Sheet opens a part the OPC rule does not name) | switch (no observable change, closes P3) |
| R3 | **Sheet and Slides `resolveRelTarget`** (only leading `../`, no `./`, `%xx` or `#` handling; Slides exact lookup) → D6 | Real files: 0 differences. Quirks: Sheet dot segments 327, fragments 327, percent-encoding 161; Slides case 347, dot segments 328, fragments 328, percent-encoding 160; never found → missing | switch to D6 |
| R1–R3 note | the one "another part" case per app (`quirk/010-percent-names`: both `word/media/image%201.png` and `word/media/image 1.png` exist; the apps take the literal `%20` name, D6 the decoded one) | artificial (Word does not write such pairs); D6 is the C# and clickCompliance validator behaviour | accept with R1/R3 |
| R4 | **Save level**: `SaveOptions::level` 6 maps to miniz_oxide level 4 (or the default becomes 4) | giant sheet (31.7 MB): miniz 6 735 ms / 2,854,910 B, miniz 4 112 ms / 2,876,909 B (+0.8 %), fflate 6 (the TS) 669 ms / 2,846,338 B; 17,798 oracle parts: miniz 6 7,273,466 B in 254 ms, miniz 4 7,285,735 B (+0.17 %) in 240 ms; `document.xml` of sw-16-8: 2,788 vs 2,790 B (fflate 6: 2,792 B). Decompressed bytes are unchanged (the parity facets hold); archive sizes change | apply: 6.5 × faster large saves for +0.2–0.8 % size |
| R5 | **Depth limit 1,000 for Sheet and Slides** (applied since D3 through `xml_part`; the TS presets had none) | deepest part: 25 in the real files, 7 in the quirks; 0 parts above 1,000. The TS parsers are recursive: docx-core stops at 1,000 (its limit), sheet-core overflows the V8 stack after 8,945 levels and slides-core after 8,353 (measured, node 24) | keep 1,000 (no real part near it; the TS ceiling is a stack accident) |

Files: `src/{flate,limits,package,zip}.rs`, `src/xml/{mod,read,tree}.rs`;
tests `package_budgets.rs`, `xml_read.rs` (`source_is_shared_not_copied` covers `String`
sources), unit tests in `flate.rs` (Deflater bytes, CRC by 8, tables) and `xml/read.rs`
(estimate); bench `tests/bench_support/mod.rs`, `examples/opc_bench.rs`,
`examples/opc_rule_evidence.rs`, `uo-wasm` feature `opc-bench` (`src/opc_bench.rs`),
`tools/bench/opc/{ts,wasm}.mjs`, `tools/bench/run.mjs` (suite `opc`, `--tag`, dirty runs named).

```
node tools/bench/run.mjs --main <built checkout of main> --suites opc [--tag <name>]
cargo run --release -p uo-opc --example opc_bench [-- <runs>]
cargo build -p uo-wasm --features opc-bench --target wasm32-unknown-unknown --profile wasm && node tools/bench/opc/wasm.mjs
UO_TS_ROOT=<built checkout of main> node --expose-gc tools/bench/opc/ts.mjs
cargo run --release -p uo-opc --example opc_rule_evidence [-- --all]
```

Checks run: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --all-features
-- -D warnings` (and on `core/fuzz`); `cargo test --workspace --release --all-features`: 212
tests pass (209 before); the parity replay: 0 mismatches; the wasm build (`count,opc-memtest`)
with `boundary.test.mjs` and `opc_memory.test.mjs`; WebKit 26.5 (`browser/run.mjs`); the
1,000-iteration native soak; the cross readers with .NET; the fuzz fallback.

## Stage 6 — final review (implementation skeptic, 2026-11-01)

The whole crate at 36c1f83d (`src/` 6,589 lines, the tests, `core/fuzz`), read against G1–G7,
D1–D8, the D5 follow-up and the stage-5 changes C1–C8. Every finding below was checked in the
code and, where marked, measured (scratch crate outside the repository,
`CARGO_TARGET_DIR=core/target/review`).

**Verdict: not ready for gate 3 until M1 is fixed** (a one-line arithmetic fix plus tests). The
should-fix items are mostly API shape that `uo-docx`, Sheet and Slides will consume; doing them
before those crates start is cheaper than after.

### Must fix before gate 3

| # | Finding | Evidence | Change |
|---|---|---|---|
| M1 | **The C7 reservation guard wraps on wasm32.** `Parser::reserve` computes the bytes to reserve in `usize` (`xml/read.rs:321-326`). On wasm32 (`profile.wasm` inherits release: no overflow checks) a count above 4 GiB wraps, passes the `≤ 3 × source` test, and every `try_reserve_exact` the host can satisfy succeeds before the first byte is parsed. A debug wasm build panics on the same input. | Strict preset with `positions: true` through `xml_part`, within the 256 MiB XML cap and the 20,000,000-element cap: 20,000,000 × `<=` followed by `=` estimates 4,854,967,296 B, which wraps to 560,000,000 B ≤ 805,306,368 B (3 × source). It reserves about 1.87 GB (elements 320 MB, positions 320 MB, nodes 160 MB, namespace tables 1.07 GB; the 2.98 GB attribute arena is refused) and then fails at offset 1. The edit helpers parse with no element cap (S1), so any preset wraps there with ~165 MB of `<=`/`=`. Native 64-bit is not affected. | Compute in `u64` with saturating arithmetic (e.g. `fn reservation_bytes(&Estimate, positions, strict) -> u64`) and compare with `len as u64 * 3`. Unit test with the wrapping counts; a peak test that a document whose counts are inflated (one root, markup after it or in a comment) reserves at most 3 × its source. Measured natively today: `<r/>` + 16 MiB of `<a b="111111111111111">` reserves 1.57 × the source (26.3 MB, freed by `finish`), and with denser markup nothing is reserved; no test pins either. |

### Should fix (before the format crates consume the API)

| # | Finding | Evidence | Change |
|---|---|---|---|
| S1 | **The edit helpers bypass D2/D3.** They parse caller text with no element or depth cap: `insert_child` (`EndOfRoot`, `BeforeFirst`) hard-codes `ParseOptions::LENIENT` (reached by `rels::add_rel`, `content_types::add_default`/`add_override`); `remove_rels`, `set_rel_target`, `remove_override`/`remove_default`, `split_element`, `parse_rels` and `ContentTypes::parse` take the caller's preset (the TS presets have no caps). A `.rels` or content-types text up to the 256 MiB XML cap that was never parsed through `xml_part` builds a tree up to ~17 × its size, where a failed `Vec` growth aborts the process (the F7 class). Each call also copies the whole text into a new `Source`. | `rels.rs:116,210,243`, `content_types.rs:56,149`, `xml/raw.rs:47,264,275` | Make the lowering of `xml_part` (`package.rs:467-472`) a public `Limits::bounded(&ParseOptions) -> ParseOptions`; every helper takes `&ParseOptions` (no hard-coded `LENIENT`) and the format crates pass the bounded options; or give `Package` the edit entry points. |
| S2 | **Stage-5 behaviour without a pinning test.** Nothing fixes C7's bound (M1) nor C3's contract: `xml_part` twice gives equal trees, counts the part once and keeps nothing (`part()` afterwards inflates again under the cap). `zip::archive_len` (a second copy of the writer's layout rules) is checked for exactness only on three non-zip64 saves (`package_budgets.rs:59,89,165`). | grep of `tests/` and `fuzz/src/lib.rs` | Add those tests; assert `capacity() == len()` in `zip_write`'s >65,535-entry test and in the `zip_roundtrip` fuzz target. Make `archive_len` `pub(crate)` unless a format crate needs it. |
| S3 | **Content-type lookup does not follow the OPC rule.** `ContentTypes::override_of` (and `remove_override`) is the Slides exact match (`content_types.rs:80-82`): case-sensitive, and `PartName` (a percent-encoded URI) is compared with the unescaped zip name. The C# Editing API that moves here compares case-insensitively (`DocxDocument.GetContentType`, `RemoveContentTypeOverride`), and D6 resolves names with ASCII folding. | `dotnet/src/UniversalOffice.Docx/Editing/DocxDocument.cs:302-341` | A `content_type_of_part` with the D6 rule (`PartName` unescaped as `resolve_relationship_target` does, ASCII folding) for the Editing API and hosts; the exact one stays the Slides parity policy (or switches with R3). |
| S4 | **Host API gaps around D3/D6.** A validator cannot see an entry's method, compressed and declared sizes through `Package` (the clickCompliance validator applies a compression-ratio rule), so it needs a second `zip::read`. XML errors from `xml_part` do not name the part (only `Limit` errors get it, `package.rs:473-476`). | — | `Package::entry(name) -> Option<&ZipEntry>` (already stored in `Content::Entry`); the part name on XML errors (field or wrapper), for the .NET `XmlException` message. |
| S5 | **Large-part edits cost 3 × the part.** The bench (d) giant peak (95 MB for the 31.7 MB sheet) and its 31.7 MB "kept" are `part_text` cached by the package + the caller's copy + the new text. Sheet edits giant sheets. | stage-6 bench, (d) giant | `Package::take_part(name) -> Result<Vec<u8>>` (moves the cached bytes out, or inflates without caching), so an edit holds the old and the new text only. |
| S6 | **CI misses two gate checks.** `m2_soak_1000` is `#[ignore]` and no job runs it (19 s release); `cargo audit` is not in `rust-core.yml`. Env-gated tests (`cfb_ts_parity`, `save_cross_readers`) report `ok` when skipped. | `.github/workflows/rust-core.yml` | A release job running the soak with `--ignored`, and `cargo audit` (or `cargo deny`, licenses too); `#[ignore = "…"]` on env-gated tests so a skip shows. |
| S7 | **Small duplications.** `rels::remove_rels` builds a `Relationship` exactly as `from_document` (`rels.rs:214-221` and `:124-133`); `open_owned` uses a literal `1100` (`package.rs:222`) next to `MAX_DEFLATE_RATIO`; `save` checks the name length (`package.rs:653`) after deflating the entry, and `add_raw` checks it again. | — | One `fn relationship(Element) -> Relationship`; the constant; check the name before planning, or rely on `add_raw`. |
| S8 | **CHANGELOG.** No 0.10 entry lists the decided behaviour changes (G5 fixes, D1 `Corrupt`, D2 and R5 limits on Sheet/Slides parts, D4, D5 raw copy keeping stored entries stored and rewriting lying ones, D7 wasm limits, the fixed zip time); the skill requires every break there. | `CHANGELOG.md` | A "0.10 (Rust core)" section, filled per module. |

### Notes (no change required now)

- **Reservation and wasm memory (C7):** inside the 3 × bound, the up-front reservation stays below
  what a real dense tree reaches by doubling (measured peak 4.5–5.3 × the source for `<a b="1"/>`
  shapes, which C7 never reserves), so no regression on 64-bit; on wasm a hostile part can raise
  `byteLength` by up to 3 × the part, permanently: the same order as the D2 tree bound.
- **`xml_part` without cache (C3):** costs CPU only (a second parse inflates again); memory and the
  total behave as documented.
- **`Send`, not `Sync`:** right for one package per request; the C# facade must serialize calls on
  a handle (say so in the `SafeHandle` wrapper). If parallel part reads are wanted later,
  `OnceLock` + `AtomicU64` make it `Sync` cheaply. `CfbFile` already uses `OnceLock` (inconsistent,
  harmless). Verified: `Package` is `Send`; `Document`, `Source` and `CfbFile` are `Send + Sync`;
  `Source` is 16 B.
- **`Source` ownership** is sound for models (refcounted, no lifetime). A kept `Source` pins the
  whole part (31.7 MB for the giant sheet), so format crates should keep it only where lossless
  saving needs positions. There is no accessor for the source span of a text or attribute value
  (only element positions); add `Source::offset_of(&str)` if a model needs one.
- **Names held three times** per opened entry (`Part.name`, `ZipEntry.name` inside
  `Content::Entry`, the index key, `package.rs:231-234`); `save` clones every name to order them
  (`package.rs:630`). Small; drop `ZipEntry.name` from the stored entry when convenient.
- `remove_part` stays O(parts) (stage-5 decision; `retain` is the bulk path).
- `safe_relative_path` returns an owned copy of its input (`bool` or `Option<&str>` would do), and
  does not check segment length (255) nor case collisions (`a.xml`/`A.xml` overwrite each other on
  Windows and macOS): document both for extracting hosts.
- **wasm32 element cap:** `Limits::WASM32` keeps the native 20,000,000 elements (~560 MB of tree at
  the cap with the hostile shape); a phone dies first. A lower wasm cap (~5,000,000, 2.5 × the giant
  sheet) is a maintainer call.
- **.NET mapping:** `LimitKind::{Entries, XmlElements}` need values in the C# `OfficeLimit`, whose
  `FieldsPerStory`/`FieldNesting` belong to `uo-docx`; decide now how `uo-ffi` maps the errors of
  several crates to one exception type.
- `Deflater::deflate_into` loops on `TDEFLStatus::Okay` with `read == 0` without a progress guard
  (`flate.rs:778-786`); with `Finish` and an accepting callback miniz returns `Done`, but a guard
  keeps termination local.
- **uo-jsnum:** besides `chars::{js_space_len, is_js_space}` (planned), `is_js_word`,
  `zip::array_index`/`js_object_order` (JS object key order: zip names and XML attributes, and the
  uo-docx writers will need it) and the `TextDecoder`/`strFromU8` decoders are JS semantics that
  belong there. `rels::next_rel_id_after_max` parses with `u64` where slides `nextRelId` uses
  `Number()`: above 2^53 or 20 digits the ids differ (TS `rId100000000000000000000`, Rust
  `rId18446744073709551615`); `to_number` + `Js` give exact parity (rels editing is not captured).
  uo-opc calls none of the `std` string methods uo-jsnum plans to disallow (no `trim`,
  `is_whitespace`, `split_whitespace`).
- **Reuse still missing** for Sheet/Slides/converter: a pull reader for very large sheets (the tree
  is 2.16 × the source) and `Package::rename_part`. The ODF/XPS/`.doc → .docx`/ODS writers and the
  rels/content-type edits have no parity capture yet (add it with those crates). ODF `mimetype`
  first relies on insertion order and no integer-like part names (as in the TS).
- **Plan vs code:** the plan placed `package`, `part_name`, `rels`, `content_types` under an `opc`
  module; they are at the crate root. Fine as is.
- **Newer Rust:** `first_chunk::<4>()`/`as_chunks::<8>()` would remove the fixed indexing of
  `cfb.rs` `u16_le`/`u32_at` and `flate.rs` `crc32_update`; `Cell::update` for the produced
  counter. Optional.

### Benchmarks main × rust-core (`tools/bench/results/2026-11-01-36c1f83d-stage6.md`)

Native numbers match stage 5 (oracle 260 ms vs TS 561 ms; giant open + parse 172 vs 686 ms;
inflate 1,175 vs 539 MB/s; docx edit + save 0.6 vs 2.4 ms). Slower than the TS: **giant edit +
save**, native 759 ms and wasm 1,289 ms against 556 ms (miniz_oxide level 6; R4), with 31.7 MB kept
after it against 3.1 MB (S5). The wasm times of this run are about 2 × the stage-5 ones (oracle
490 vs 245 ms, inflate 580 vs 1,133 MB/s) while the allocation counts are identical: this review's
release builds and test runs shared the machine during the run, so re-run the wasm column alone
before accepting the numbers.

### Proposals R1–R5

- **R1 (Write target rule → D6): approve.** One real document changes (custom XML parts found), no
  layout effect, and it is the rule of the C# port and of the clickCompliance validator. Name the
  acceptance in `opc_parity.rs` like D1/D4.
- **R2 (Sheet Unicode → ASCII folding): approve.** No observable difference; closes security P3.
- **R3 (Sheet/Slides `resolveRelTarget` → D6): approve,** with S3 so content types follow the same
  rule.
- **R4 (save level): approve the goal, not the remapping.** Mapping `level: 6` to miniz level 4
  makes the number lie (`deflate_raw`, `Deflater` and `zlib` take the same level); set
  `WriteOptions::default().level = 4` (or a named enum) and keep levels as miniz levels.
  Re-measure wasm; `uo-pdf` chooses its own level.
- **R5 (depth 1,000 for Sheet/Slides): approve** (keep), and list it in the CHANGELOG (S8).

### Checked and fine

G1–G7 and D1–D8 each found in the code with a test that pins them (`flate::empty_stream`;
`push_element` + `Limits::max_xml_elements`; `Package::xml_part`; the D4 acceptance in the
replay; `Package::intact` + `overlapping`, whose sweep is right for nested and chained overlaps,
and the `MAX_DEFLATE_RATIO` bound, which holds because every symbol costs at least one bit, so at
most 1,032:1; `resolve_target`; `Limits::WASM32`; `safe_relative_path`). No `unsafe` (`forbid`);
no `unwrap`/`expect`/`panic` outside tests (workspace lints); the 63 indexing and 14 slicing sites
reported by `clippy::indexing_slicing` are behind length checks or at ASCII positions; the 14
`u64 → usize` casts on wasm32 are bounded by the input or by the 64 KiB window; no recursion on
data (CFB walk ≤ 32 levels); `Error::Limit` carries what the C# `OfficeLimitExceededException`
needs (kind, max, part). `cargo test -p uo-opc --release`: all pass (parity replay: 10 tests, 0
mismatches); `cargo fmt --check` and `cargo clippy -p uo-opc --all-targets --all-features -D
warnings` clean; `cargo audit` (1,395 advisories): 0 for the 7 crates of `core/`;
`miniz_oxide =0.9.1` is the latest release (`MIT OR Zlib OR Apache-2.0`), `adler2 2.0.1`
(`0BSD OR MIT OR Apache-2.0`); the 70 files of the crate are LF. Not re-run here: wasm/WebKit, the
cross readers, fuzzing (the record's runs stand).

Process: implementation skeptic (Opus), about 35 min, ~55 tool calls.

## Stage 6 findings — handled (2026-11-01)

Every item of the final review except R1–R5 (result-changing, the maintainer decides; none is
applied). Each change has tests that failed before it (an assertion of the old behaviour, a
measured bound, or an API that did not exist); results within the limits are unchanged (parity
replay: 0 mismatches against the committed capture).

| # | What changed | Files | Tests and numbers |
|---|---|---|---|
| M1 | `Parser::reserve` asks `Estimate::fits`, which computes the arena bytes in `u64`, saturating (`Estimate::bytes`), and compares them with `len as u64 × 3` (`RESERVE_PER_SOURCE_BYTE` is a `u64`). The finding's input on wasm32 now reserves nothing | `src/xml/read.rs`; `uo-wasm/src/opc_memtest.rs` (test-only export `uo_opc_memtest_parse_strict`: strict + positions, `Limits::DEFAULT.bounded`) | unit `xml::read::tests::the_estimate_does_not_wrap` (the counts of the 256 MiB part of 20,000,000 × `<=` then `=`: 4,854,967,296 B, which is 560,000,000 B mod 2³² and passed the old bound; saturating counts); `xml_memory::reservation_never_exceeds_three_times_the_source` (16 MiB inputs with inflated counts, four presets: peak 1.57 × lenient, 2.26 × with positions, 2.61 × strict with positions, 0 for denser markup and the M1 shape; asserted ≤ 3 × + 64 KiB); wasm `opc_memory.test.mjs` "M1" (Node and WebKit): **before +2,142,371,840 B of wasm memory (7.98 × the source), after +268,500,992 B (1.00 ×: the source itself)** |
| M1 audit | The 14 `u64 → usize` casts re-checked after the changes: inflate back-reference distance (≤ 32,768) and length (≤ 258), the 64 KiB window ring (masked) and its CRC flush (≤ 64 KiB), stored-block range (≤ the input), the first inflate reservation (≤ 64 MiB), zip data ranges (clamped to the archive), CFB sector sizes. The guards are all `u64` (zip entry count ≤ len / 46, CFB, inflate caps, `archive_len`). Three capacity hints multiplied counts in `usize` and could wrap on wasm32 into a `with_capacity` above `isize::MAX` (a panic, an abort in wasm): `rels::write_rels` (`rels × 128`), `part_name::escape_segment` (`len × 3`), `xml::write::el` (`name × 2`); now saturating with `try_reserve_exact` (a hint the host refuses is skipped; the same capacity otherwise). The other hints add lengths of strings in memory (each ≤ `isize::MAX`, so the sum fits) | `src/rels.rs`, `src/part_name.rs`, `src/xml/write.rs` | — |
| S1 | `Limits::bounded(&ParseOptions) -> ParseOptions` (public: the D2/D3 lowering `xml_part` uses) and `Limits::parse` (crate): a text longer than `max_xml_part_bytes` is `Limit { PartBytes }` before it is copied, then the bounded parse. Every edit/parse helper takes the package's `&Limits`: `xml::raw::insert_child` (no hard-coded `LENIENT` limits: names as written, positions, within the limits) and `split_element`; `rels::parse_rels`, `add_rel`, `remove_rels`, `set_rel_target`; `ContentTypes::parse`, `content_types::add_default`, `add_override`, `remove_override`, `remove_default` (and the new `remove_overrides_of_part`). The helpers that take a preset keep it (bounded); callers pass `pkg.limits()` | `src/limits.rs`, `src/xml/raw.rs`, `src/rels.rs`, `src/content_types.rs`, `src/package.rs`; callers in the tests, `fuzz/src` (`FUZZ_LIMITS`), `uo-wasm/src/opc_memtest.rs` | `stage6_findings::s1_bounded_lowers_only_the_limits` (lowered, kept when lower, the same tree within the limits), `s1_every_edit_helper_refuses_a_text_over_the_limits` (the 12 helpers: `XmlElements` at 1,001 elements, depth, `PartBytes` with **0 B allocated**; one element less gives the unbounded results; the text-only anchors unchanged), `s1_a_hostile_text_is_a_limit_not_an_allocation` (1 MiB of `<a/>` through `add_override`: **peak 1.04 × the text with the element cap, 21.00 × without**) |
| S2 | Pinning tests for stage 5; `zip::archive_len` is `pub(crate)` (only `Package::save` uses it) | `src/zip.rs`; `fuzz/src/lib.rs` | `stage6_findings::s2_xml_part_keeps_nothing_and_counts_the_part_once` (C3: equal trees, `produced_bytes` = the part once, a parse and its drop leave 0 B live, `part` afterwards inflates again and is not counted again, a part over the cap is refused by both reads each time); C7 by the M1 tests; `zip::tests::archive_len_is_the_written_length` (empty, stored and deflated with UTF-8 names, declared sizes of 4 GiB and above, 65,536 entries: length and capacity equal; offsets of 4 GiB are not built); `zip_write::zip64_when_more_than_65535_entries` also saves the 65,536 entries through `Package` (levels 0 and 6: `capacity() == len()`, level 0 byte-identical to `zip::write`); the `zip_roundtrip` fuzz target asserts `capacity() == len()` on every save |
| S3 | `ContentTypes::override_of_part` / `content_type_of_part` (the D6 rule: each `PartName` resolved like an absolute target by `part_name::resolve_relationship_target`, `%XX` unescaped once, malformed names match nothing; ASCII case folding; the default by the extension of the last segment) and `content_types::remove_overrides_of_part` (every match, as the C# `RemoveContentTypeOverride`). The Slides exact rule is renamed `override_of_exact` / `content_type_of_exact` (`remove_override` stays exact and says so) | `src/content_types.rs`; `fuzz/src/lib.rs` (the `content_types` target calls them and checks that a generated override is found in any ASCII case) | `stage6_findings::s3_content_type_of_a_part_follows_the_opc_rule` (case, `%20`, `%E2%82%AC`, malformed `%ZZ`, defaults of the last segment, removal of both case variants) |
| S4 | `Package::entry(name) -> Option<&ZipEntry>` (exact name: method, flags, CRC-32, declared compressed and uncompressed sizes; `None` for parts set by the caller). `Error::Xml` gains `part: Option<String>`, set by `xml_part` (as `Limit` already was) and shown by `Display`; the variant is the same, so the error classes do not change (the replay compares XML errors as `E`, zip errors by `error_class`) | `src/package.rs`, `src/error.rs`, `src/xml/{read,raw}.rs` | `stage6_findings::s4_entry_exposes_what_the_central_directory_declares`, `s4_xml_errors_from_xml_part_name_the_part` |
| S5 | `Package::take_part(name) -> Result<Option<Vec<u8>>>`: an entry's inflated copy is moved out (0 B allocated), a never-read entry is inflated for the caller without caching (counted once), a stored one copied from the input; the entry stays readable from the archive. Bytes set by the caller are moved out and the part is empty until the next `set_part` (same position). The bench edit (d) uses it | `src/package.rs`; `tests/bench_support/mod.rs` | `stage6_findings::s5_take_part_hands_the_bytes_over`, `s5_an_edit_of_a_large_part_holds_it_twice` (4.9 MB part: **edit peak 3.00 × → 2.00 ×**). Bench (native, `opc_bench`): **giant edit + save peak 95,153,899 → 63,435,928 B (−33%)**, allocated 131,840,749 → 100,122,802 B, 782.6 → 748.6 ms; kept 31,718,822 B unchanged (the new text, owned by the package); sw-16-8 edit + save peak 703,177 → 514,907 B, allocated 1,498,332 → 1,310,062 B; scenarios (a)–(c) identical. The wasm bench was not re-run |
| S6 | CI: job `soak` (`cargo test -p uo-opc --release --all-features --test package_soak -- --ignored m2_soak_1000`) and job `audit` (`cargo audit` on `core/Cargo.lock` and `fuzz/Cargo.lock`, installed with `taiki-e/install-action`). The opt-in tests stay plain tests (the documented commands do not pass `--ignored`) but call `uo_testkit::opt_in(var, what)`, which writes `SKIPPED: <what>: set <var>=1` to the process's stderr past libtest's capture: `cfb_ts_parity`, `save_cross_readers` and its .NET half | `.github/workflows/rust-core.yml`, `uo-testkit/src/lib.rs`, `tests/{cfb_ts_parity,opc_parity}.rs` | the two SKIPPED lines show in `cargo test --workspace`; `cargo audit` here: 0 of 1,395 advisories for the 7 crates of `core/` and the 14 of `fuzz/` |
| S7 | `rels::remove_rels` builds each `Relationship` with the function of `from_document`; `open_owned` filters with `MAX_DEFLATE_RATIO`; `save` checks the name length first, before anything is compressed | `src/rels.rs`, `src/package.rs` | `stage6_findings::s7_a_name_too_long_fails_the_save_before_compressing` (an 8 MiB part under a 65,536-byte name: `NameTooLong` with < 256 KiB allocated; before, the part was deflated first) |
| S8 | `CHANGELOG.md`: "Unreleased — 0.10 (Rust core)", container layer: the G5 fixes, D1, D2 and the XML cap of D3, the depth limit on Sheet/Slides parts (R5, marked pending), D4, D5 with the overlap rule and stored entries kept stored, D7, the fixed zip time, zip64, and the internal API changes | `CHANGELOG.md` | — |
| note | `Deflater::deflate_into`: no real infinite-loop risk. With `TDEFLFlush::Finish` and a callback that takes every chunk, miniz_oxide's callback output reports `flush_remaining` 0 to its block loop, so `compress_normal`/`compress_fast`/`compress_stored` consume the whole input, the final `flush_block` sets `finished`, and the call returns `Done`: the `Okay` arm was unreachable. The loop is replaced by one call that is an error unless `Done` with the whole input read, so there is nothing to guard | `src/flate.rs` | `flate::tests::deflater_writes_the_bytes_of_deflate_raw` now also covers level 0 (stored blocks): levels 0–9 × 11 inputs up to 400 KB, one call each; the 31.7 MB giant sheet saves in the bench and `package_budgets` |

API changes (crate not published): `Limits::bounded`; `&Limits` on the 12 edit/parse helpers;
`ContentTypes::{override_of_part, content_type_of_part}`, `content_types::remove_overrides_of_part`;
`override_of` → `override_of_exact`, `content_type_of` → `content_type_of_exact`;
`Package::{entry, take_part}`; `Error::Xml { part }`; `zip::archive_len` crate-private;
`uo_testkit::opt_in`. Wasm module: `opc-memtest` 266,269 → 268,226 B, `count,opc-memtest`
266,760 → 268,717 B (the M1 export); the Safari 15 feature check passes.

Not done here: R1–R5 (maintainer); the wasm column of the benchmark (the review asked for a
re-run on a quiet machine, and S5 changes its (d) numbers too); the review's notes that need no
change now (`Send` not `Sync`, names held three times, `remove_part` O(parts),
`safe_relative_path` docs, the wasm32 element cap, the .NET error mapping, uo-jsnum, newer Rust
APIs); the edit helpers still copy the caller's text into the tree's source (one copy per call,
now bounded by the XML cap). "Process metrics" has no row for this round yet (the orchestrator
times the agents).

### Checks run

`cargo fmt --all --check`; `cargo clippy --workspace --all-targets --all-features -- -D
warnings` (and in `core/fuzz`); `cargo test --workspace --release --all-features`: **225 tests
pass** (212 before; 1 ignored, the soak), the parity replay 0 mismatches; the 1,000-iteration
native soak: 771,375 B × 21 flat, 12.5 s; the wasm build (`count,opc-memtest`) with
`boundary.test.mjs` and `opc_memory.test.mjs` in Node (M2 1,000 iterations: 0 B live,
`byteLength` 2,293,760 B flat, 12.9 s) and in WebKit 26.5 (`browser/run.mjs`, 15.3 s); the fuzz
fallback, 10 s per target: 0 crashes (zip_open 91,842 execs, zip_roundtrip 234,955, zip_write
161,210, inflate 511,823, cfb 4,628,999, xml_lenient 763,986, xml_strict 813,839,
escape_roundtrip 279,843, raw_surgery 1,328,853, rels 226,448, content_types 201,933, caseless
109,085, part_text 1,648,196, resolver 3,658,295); the cross readers (`UO_TS_PARITY=1
UO_DOTNET_PARITY=1`): 10,053 saved archives read identically by fflate, uo-opc and .NET
`ZipArchive`; `cargo audit`. The new tests take under 7 s per file in debug.

New test file: `tests/stage6_findings.rs`.

## Gate 3 — decision (approved 2026-11-01)

Maintainer, verbatim: "Eu aprovo tudo" — every final recommendation accepted.

- R1 Write's relationship rule → D6 (accepted difference: `site/assets/samples/demo.docx` customXml targets now found).
- R2 Sheet case folding → ASCII.
- R3 Sheet and Slides `resolveRelTarget` → D6, together with S3 (D6 content-type lookup).
- R4 the final skeptic's version: the default save level becomes 4; an explicit level keeps meaning that miniz level.
- R5 keep the depth limit of 1,000 for Sheet and Slides (CHANGELOG).

## Gate 3 decisions — implemented (2026-11-02)

All five decisions are in the crate. Parity differences they cause are accepted by name in the
replay, with counts pinned to the stage-5 evidence; no comparison was loosened. The capture
(`tests/parity/opc.cap`, the frozen TS) is unchanged.

### What the format crates use from now on

- **Relationship targets: D6 only.** `uo-docx`, Sheet and Slides resolve every relationship target
  with `Package::resolve_target(source_part, target)` (the OPC rule of the C# `OpcPartName`, then
  the exact name, else ASCII case folding) and open the name it returns. The 0.9.x rules (Write
  `./` stripped, `/x` → `x`, else `word/${target}` with an exact lookup; Sheet/Slides
  `resolveRelTarget` with a Unicode-lowercase or exact lookup) are gone from the policies of
  `tests/parity_support/mod.rs`; `parity_support::legacy` reproduces them only to explain the
  captured TS in the replay and in `examples/opc_rule_evidence.rs`. Content types: the D6 lookup
  `ContentTypes::content_type_of_part` (S3); `content_type_of_exact` stays only where a crate
  must match Slides 0.9 exactly.
- **Case folding: ASCII only.** `CaseFold` is `None` (Write, Slides, ODF) or `Ascii` (Sheet).

### R1 / R3 — the D6 path for Write, Sheet and Slides

- `parity_support`: `App::resolve(pkg, owner, target)` is the D6 path for the three apps
  (`RuleOutcome { name, found }`, streamed as `R` + the resolved name, or `U` when the target names
  no part, then `F` + exact / `N`, the capture's encoding). `rels_value` also streams the facet with
  `legacy::resolve` and lists every relationship whose outcome differs, classified by decision
  (`Rule::{R1, R2, R3}`: Write → R1; Sheet with the same resolved name → R2; else R3) and by
  outcome change.
- `opc_parity.rs`: a `rels` facet that differs is accepted only when the 0.9.x rules give exactly
  the captured value; the reason names the decisions (`R1`, `R3`, `R2`). Each test pins the
  relationships changed per decision and, for real files, the inputs; every test also asserts that
  no relationship goes from found to missing.

| Share | R1 | R2 | R3 | Inputs |
|---|---|---|---|---|
| oracle docx (4 tests), sheet samples, slides samples | 0 | 0 | 0 | none |
| other fixtures | 2 (missing → found) | 0 | 0 | `site/assets/samples/demo.docx`: `../customXml/item1.xml`, `item2.xml` (Word wrote them; 0.9 looked for `word/../customXml/…`) |
| quirk corpus | 2,059 (851 missing → found, 1 another part, 1,207 another resolved name with the same outcome) | 0 | 7,568: Sheet 3,421 (825, 1, 2,595), Slides 4,147 (1,177, 1, 2,969) | 171 Write and 353 Sheet/Slides `rels` facets |

- The evidence tool now reads the packages the replay compares (the six that D1 stops at load are
  taken as fflate holds them, `load_as_fflate_d1`): quirk corpus 532 packages and 22,974
  relationships (stage 5: 526 and 22,704), missing → found Write 851 (case 178, fragment 170,
  parent segment 338, percent-encoding 165), Sheet 825 (dot segment 331, fragment 331,
  percent-encoding 163), Slides 1,177 (case 351, dot segment 332, fragment 332, percent-encoding
  162), one "another part" per app (`quirk/010-percent-names`), and per app the relationships whose
  resolved name or part differs (Write 2,059, Sheet 3,421, Slides 4,147): the replay's counts.
  Real files unchanged: 2,819 packages, 9,289 relationships, the two `demo.docx` targets.

### R2 — Sheet folds ASCII

- `CaseFold::Unicode` removed (no user left). `Package` keeps one ASCII index instead of two: the
  fallback of `find` under `CaseFold::Ascii` and of `resolve_target` (`Package::folded`).
- Sheet's open options in the parity policy, the memory support, the wasm `opc-memtest` export and
  the fuzz targets use `CaseFold::Ascii`; the `caseless` fuzz target checks the two policies
  against its model.
- Parity: 0 differences (no `load`, `edit` or `rels` facet of any share changed; the R2 count is 0
  in every test). Evidence tool: 0 Sheet lookups where Unicode and ASCII disagree, in real files
  and quirks.
- Tests: `gate3_decisions::r2_sheet_folds_ascii_only` (`É`/`é` and U+212A not folded, ASCII case
  folded, the same index answering `resolve_target`); `gate2_decisions::d6_*` (no policy opens the
  KELVIN SIGN target); `cache_caps::m5_caseless_index` (now through the ASCII index) and
  `m5_ascii_index_of_resolve_target` (one index for both lookups); `package_ops` and `api_memory`
  over the two policies.

### R4 — default save level 4

- `zip::DEFAULT_LEVEL = 4`; `WriteOptions::default()` (and so `SaveOptions::default()`) uses it. An
  explicit `level` is passed to miniz_oxide as is, as by `flate::deflate_raw`, `Deflater` and
  `zlib` (the `deflate_raw` doc said "fflate levels"; now "miniz_oxide level").
- Test: `gate3_decisions::r4_default_level_is_4_and_a_level_is_the_miniz_level` (default 4; for
  levels 1–9 the deflated stream of `Package::save`, `ZipWriter` and `zip::write` is
  `deflate_raw(data, level)`; levels 4 and 6 differ on the sample; the contents read back equal).
- Numbers (same machine, back to back; native `opc_bench`, best of 5):

| | level 6 | level 4 (default) |
|---|---|---|
| giant edit + save, native | 789.1 ms, 2,856,998 B | **162.3 ms, 2,878,989 B (+0.77%)** |
| giant edit + save, allocated / peak | 100,122,802 / 63,435,928 B | 100,169,716 / 63,435,928 B |
| giant edit + save, wasm (`tools/bench/opc/wasm.mjs`) | 941.1 ms (gate-3 bench run) | **193.4 ms**, 2,878,989 B |
| TS (V8, fflate 6; gate-3 bench run) | 556 ms, 2,848,422 B | — |
| sw-16-8 edit + save | 5,358 B | 5,360 B |
| 17,798 oracle parts, `deflate_raw` each | 7,273,466 B in 253.3 ms | 7,285,735 B (+0.17%) in 238.3 ms |
| the 2,742 oracle docx rewritten and saved | 9,335,444 B | 9,347,713 B (+0.13%) |

  The giant edit + save is now faster than the TS (162 / 193 ms against 556 ms); the other
  scenarios are unchanged (oracle 240.4 ms, giant open + parse 175.6 ms, inflate 28.2 ms native).
- M3 budgets: only `giant-1m save edited sheet` moved, by the larger output (2,856,819 →
  2,878,818 B): allocated 11,017,000 → 11,064,000 B, peak 6,627,000 → 6,662,000 B (recorded in
  `package_budgets.rs` with the reason). Every other budget measured as before (deck edit + save
  17,768,362 B, sw-16-8 edit + save 2,659,849 B, giant save untouched 3,297,440 B). No time budget
  exists in the tests.

### R5 — depth 1,000 for Sheet and Slides

- Unchanged behaviour (in place since D3); now pinned by
  `gate3_decisions::r5_depth_limit_of_1000_for_sheet_and_slides`: `Limits::{NATIVE, WASM32,
  DEFAULT}.max_xml_depth == 1_000`; through `xml_part` the `sheet`, `sheet-chart`, `slides` and
  `docx-pos` presets parse 1,000 levels and refuse 1,001 with `Limit { XmlDepth, max: 1000, part }`;
  the Sheet/Slides presets alone have no limit (1,001 levels parse). Evidence tool: deepest part 25
  (real files) and 7 (quirks), none above 1,000. The CHANGELOG line is no longer "pending".

### CHANGELOG

"Unreleased — 0.10 (Rust core)", container layer: R5 marked decided; new entries for R1/R3 (one
relationship rule for the three apps, the `demo.docx` example, no part lost), R2 (ASCII folding)
and R4 (level 4 by default, 160 vs 790 ms, +0.8% / +0.13–0.17%, an explicit level is the
miniz_oxide level); internal API: `CaseFold::Unicode` gone, `zip::DEFAULT_LEVEL`.

### Checks run

- `cargo fmt --check` (`-p uo-opc -p uo-wasm`, and `core/fuzz`): clean.
- `cargo clippy -p uo-opc -p uo-testkit -p uo-wasm -p uo-alloc -p uo-ffi --all-targets
  --all-features -- -D warnings` and `cargo clippy --all-targets -- -D warnings` in `core/fuzz`:
  clean. The workspace-wide clippy fails only in `crates/uo-jsnum` (stage-3 port in progress by
  another agent, not touched here).
- `cargo test --release --all-features -p uo-opc -p uo-testkit -p uo-wasm`: 225 pass, 1 ignored
  (the soak); the parity replay 0 mismatches, accepted by name G4, D1, D4, R1 (1 + 171 facets) and
  R3 (353 facets). The 1,000-iteration native soak: 771,375 B × 21 flat.
- Wasm (`count,opc-memtest`, 268,368 B, was 268,717 B; the Safari 15 feature check passes):
  `node --expose-gc --test crates/uo-wasm/tests/boundary.test.mjs` 4/4,
  `node --test crates/uo-wasm/tests/opc_memory.test.mjs` 5/5 (M2 1,000 iterations: 0 B live,
  `byteLength` 2,293,760 B flat, 13.1 s; M1 +268,500,992 B, 1.00 × the source). `opc-bench`
  module 288,568 B (was 288,917 B).
- Fuzz fallback, 10 s per target: 0 crashes (zip_open 82,076 execs, zip_roundtrip 224,962,
  zip_write 149,637, inflate 500,223, cfb 4,421,946, xml_lenient 729,211, xml_strict 788,698,
  escape_roundtrip 273,731, raw_surgery 1,293,215, rels 216,236, content_types 191,996, caseless
  120,746, part_text 1,609,845, resolver 3,503,993).
- Cross readers (`UO_TS_PARITY=1 UO_DOTNET_PARITY=1`, `UO_TS_ROOT` = the clickcompliance-docx
  checkout): 10,053 saved archives read identically by fflate, uo-opc and .NET `ZipArchive` (the
  saves now at level 4).

Not run here: the WebKit runner (`browser/run.mjs`) and real Safari; a full
`tools/bench/run.mjs --main …` table (the Rust runners were re-run, the TS column is the gate-3
run's). "Process metrics" has no row for this round (the orchestrator times the agents).

Files: `src/{package,zip,flate}.rs`; tests `gate3_decisions.rs` (new), `opc_parity.rs`,
`parity_support/mod.rs`, `gate2_decisions.rs`, `cache_caps.rs`, `package_ops.rs`,
`package_budgets.rs`, `api_memory.rs`, `package_memory.rs`, `memory_support/mod.rs`;
`examples/opc_rule_evidence.rs`; `uo-wasm/src/opc_memtest.rs`; `fuzz/src/lib.rs`; `CHANGELOG.md`.

## Stage 5 — evolution: the JS helpers moved to uo-jsnum (uo-jsnum D2, 2026-11-03)

Simplification and algorithms specialist (Opus 5.5), worktree `rust-core`, `CARGO_TARGET_DIR=core/target/stage5-b`.
The uo-jsnum items of the stage-6 notes ("Reuse", above): uo-opc now depends on `uo-jsnum` and keeps no copy of a JS
semantic. Result-preserving; the table of the whole move (with uo-fonts' `js` module) is in `uo-jsnum.md`, "Stage 5 —
evolution".

| uo-opc before | Now |
|---|---|
| `xml/chars.rs` `js_space_len`, `is_js_space`, `is_js_word` (`pub(crate)`) | `uo_jsnum::text::{js_space_len, is_js_space, is_js_word}` (used by `xml/read.rs`, `xml/raw.rs`, `xml/entities.rs`); `chars.rs` keeps the XML 1.0 classes only |
| `zip::array_index` (`pub(crate)`, also used by `xml/read.rs`), `zip::js_object_order` | `uo_jsnum::object::{array_index, js_object_order}`; `uo_opc::zip::js_object_order` is a re-export (API unchanged) |
| `zip::decode_utf8`, `zip::decode_latin1` | `uo_jsnum::text::{decode_utf8, decode_latin1}`, re-exported from `uo_opc::zip` (API unchanged) |
| `package::decode_text`, `decode_text_owned` | unchanged: the UTF-16 BOM option and the in-place BOM removal are uo-opc's |
| `rels::next_rel_id_after_max` (`u64` where slides `nextRelId` uses `Number()`) | unchanged: `to_number` would change results above 2^53 (no capture covers rels editing), a maintainer decision, not part of D2 |

**Numbers.** Wasm (`--profile wasm`): `opc-memtest` 267,877 → 255,310 B, `count,opc-memtest` 268,368 → 255,801 B,
`opc-bench` 288,568 → 276,001 B (−12,567 B each, all in the code section: 242,372 → 229,801 B; the data section is
unchanged; inlining differences, the cause was not traced). Time, `examples/opc_bench` (best of 7, two alternated runs;
ms): corpus 229.5/228.7 → 228.4/226.3, giant 148.8/151.1 → 146.0/146.5, inflate 27.1/27.3 → 26.9/26.8, save docx 0.4
→ 0.4, save giant 157.2/155.8 → 156.6/156.3; wasm in Node 24.15 (`tools/bench/opc/wasm.mjs`, best of 5): corpus
238.6/239.9 → 237.9/238.6, giant 170.9/165.8 → 167.5/167.9, inflate 28.2/28.0 → 28.1/28.2, save giant 189.8/190.0 →
191.0/189.9. Equal within the noise.

**Parity and tests.** `opc_parity` 0 mismatches in every group: oracle docx 5,328 + 5,520 + 5,472 + 5,616 equal, the
quirk corpus 3,545 equal and 795 accepted by name (as before), other fixtures 313 equal and 11 accepted, slides 328 and
52 accepted, sheet 56; no capture file changed. Every uo-opc test passes (`cargo test --release --workspace
--all-features`, including `zip_write::js_object_order_puts_array_indices_first` through the re-export);
`opc_memory.test.mjs` 5/5 in Node with the new `count,opc-memtest` module (M2 1,000 iterations flat); the new modules
validate with the Safari 15 feature list of CI. The JS helpers are now tested where they live: the V8 whitespace set
over every code point and `js_space_len` against it, `\w` and the object key order by definition (`uo-jsnum/tests/
text_v8.rs`), their allocation in `uo-jsnum/tests/memory_gate.rs`.

**Process metrics.** Stage 5 (D2 for uo-opc), simplification and algorithms (Opus 5.5): one session with uo-jsnum D2
and uo-fonts D5, about 80 minutes and about 120 tool calls for the three; rework: none in uo-opc.
