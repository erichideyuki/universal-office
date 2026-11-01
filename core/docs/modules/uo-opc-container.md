# uo-opc — the document container layer

Record of the rust-port workflow. Status: **gate 1 approved 2026-11-01**; stage 3 (port) done; stage 4 (verification) next.

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
