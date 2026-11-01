# uo-opc — the document container layer

Record of the rust-port workflow. Status: **gate 1 approved 2026-11-01**; stages 3 (port) and 4 (verification) done; **gate 2 pending** (decisions D1–D8 below).

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

## Gate 2 — summary and decisions (pending)

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
