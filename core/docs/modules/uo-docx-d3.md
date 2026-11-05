# uo-docx D3 — the Editing API (architecture, port, parity, security)

Brief: `uo-docx.md`, module table row **D3**, "D3 — Editing API" (Data model), U13, G13, G27, the D3 rows of
"Parity", "Memory" and "Security". Code: `crates/uo-docx/src/api/**` (feature `editing-api`), the C#
differential harness `crates/uo-docx/dotnet-harness/`, tests `tests/parity_d3*.rs`, `tests/memory_d3.rs`,
`tests/nopanic_d3.rs`, captures and fixtures `tests/parity/d3/`. Source of behaviour: the frozen C#
`dotnet/src/UniversalOffice.Docx/Editing/**` and `CustomInlineTag.cs` (identical to `main`; ≈ 4,750 lines),
run by the harness on .NET 10.0.5. Opus, one session, 2026-11-05 (architecture, DOM, strict reading,
serializer, callbacks, the API translation and the security review: the plan's Sonnet share was done in the
same session, inside the architecture written first).

**Status**: the whole C# Editing API is ported (every public type and member, the internals they use) and
compiles with `clippy -D warnings` (native and wasm32). Parity: 100% on the .NET-semantics capture (18,068
records, 21 accepted by name) and on the corpus capture (see "Parity results"). Memory gate M1/M2/M3/M5
green; no-panic tests green; the fuzz target body is in `core/fuzz/src/d3.rs`. Nothing committed.

## 1. Architecture

### 1.1 DOM (`api::dom`)

The C# works on System.Xml.Linq; the port is a mutable arena with the XLinq semantics the saved bytes and the
API results depend on — not a 1:1 object graph (no `Rc<RefCell>`).

- **One arena per `DocxDocument`** (`Dom`): every parsed part is a `Document` node of it; detached subtrees
  (copies being built, the row template, removed content) live in the same arena. Slots are **never
  reused**; a `NodeId { stamp, index }` carries the arena's stamp (a process-wide `AtomicU32` serial), so an id
  of another document, or forged, is refused (`InvalidOperation`), never UB and never another node. This is
  the "generational" id of the brief with the generation per document: within a document a slot cannot be
  stale because it is never freed. Detached nodes live until the document is dropped — the C# GC frees them at
  some point and nothing observable depends on when; the arena is bounded by the package limits plus the
  work done on it (record "Memory"). One arena (not one per part) because the C# moves and copies nodes
  between parts of a document freely (samples, section merges, notes); copies between two documents (the
  importer: content → template) go through `Dom::import`, which re-interns names.
- **Names**: interned `(namespace, local)` pairs (`Name`, `Ns`). The vocabulary of the API (≈ 400 names, the
  C# `W.*`, `Ns.* + …`, the schema-order lists) is pre-seeded in a fixed order (`dom::n`), so every arena gives
  it the same ids and the code compares integers. Namespace declarations are attributes, as in XLinq:
  `xmlns:p` is `{XMLNS}p`, the default `xmlns` is `{}xmlns`; prefixes are not stored — the serializer derives
  them, as XLinq does.
- **Content forms**: an element with no child nodes is *empty* (`<a/>`, XLinq `content == null`, written
  `<a />`) or *open* (`<a></a>`, `content == ""`); the reader keeps the form, `Value = ""`, `Add("")`,
  `RemoveNodes()` and removing the last child follow XLinq (`AddString`, `RemoveNodes`, `RemoveNode`).
- **Insertion clones**: adding a node that has a container (or that is the top of the target's own tree)
  inserts a deep copy and leaves the original in place (XLinq `AddNode`/`Inserter`). Several C# paths depend
  on it (`ReplaceWith(e.Nodes())` while `e` is detached keeps working on the detached tree; `DropReferences`
  leaves the attached copy of a nested hyperlink). Text strings merge into a preceding text node (not CDATA).
- **Dirty**: every mutation that raises an XLinq `Changed` event on an attached node marks its part (the
  `Document` node) dirty — attribute set (even to the same value), add, remove, `Value`, `RemoveNodes` of
  non-empty content, `Add("")` on an empty element; mutations of detached nodes do not. Dirty parts are what
  `Save` re-serializes; a non-dirty part leaves byte for byte.
- Every walk is iterative (explicit stacks); `descendants`, `elements_named`… return snapshots, which is what
  the C# `.ToList()` gives; the few lazy C# enumerations that matter are lazy here too
  (`following_elements`).

### 1.2 Strict reading (`api::read`)

C# `DocxDocument.Parse`: `XmlReader` (DTD prohibited, no resolver, whitespace/comments/PIs kept) with the
depth checked before the tree, then `XDocument.Load(…, PreserveWhitespace)`.

- The element tree comes from the uo-opc strict parser through `Package::xml_part` (the only way to parse a
  part, D3 of uo-opc): namespaces resolved, close tags checked, duplicates (also by expanded name) refused,
  predefined entities only, char references checked, `xml:space` checked, depth = `XmlReader.Depth` (root 0)
  against `DocxLimits::max_xml_depth`, plus the package's part caps and element cap.
- The XML declaration and the nodes outside the root (which that tree does not keep) are read here from the
  same text with the `XmlReader` rules: `version="1.0"` first, then `encoding`, then `standalone` (`yes`/`no`);
  prolog/epilog comments, PIs and white space become top-level nodes (line ends normalized), as XLinq keeps
  them.
- Empty vs open form from the element positions (`content_start == end` is `<a/>`).
- Text decoding: UTF-8 (BOM removed) or UTF-16 with a BOM (`OpenOptions::utf16_text`). A part declaring
  another encoding is read as UTF-8 (accepted by name, §7).

### 1.3 Serializer (`api::write`)

A port of what `Serialize` runs, measured byte for byte against .NET 10 on the corpus (every reserialized part
of the 2,802 documents × 18 scripts):

1. `EnsureNamespaceDeclarations(root)`: every namespace used by an element or a non-declaration attribute,
   in first-use order (the `HashSet<XNamespace>` enumeration after removals only = insertion order), that the
   root neither declares with a prefix nor as its default gets `xmlns:<canonical prefix>` (the Word table, or
   `ns`, then `ns1`, `ns2`… while the root has that attribute) appended to the root; the Word extension
   namespaces (`…office/…/word/20…` except wps/wpg/wpc/wpi) go into `mc:Ignorable` (`xmlns:mc` added if the
   root does not declare `mc`; a root that binds `mc` to another URI makes the add a duplicate:
   `InvalidOperation`, as in C#). The DOM is mutated (and stays so), as in C#.
2. `XDocument.Save(XmlWriter)` with `Encoding = UTF-8 without BOM, Indent = false, NewLineHandling = Entitize,
   OmitXmlDeclaration = false, CheckCharacters = true`, in three layers as in .NET:
   - **XLinq `ElementWriter`/`NamespaceResolver`**: the prefix of a name is the newest declaration in scope
     that is not shadowed (attributes skip the default namespace), else `xml`/`xmlns` by name, else none.
   - **`XmlWellFormedWriter`**: its own namespace stack with the predefined `xmlns`, `xml` and empty
     bindings; `PushNamespaceImplicit` (NeedToWrite / Implied / redefinition errors), explicit declarations
     written in attribute order (`PushNamespaceExplicit`; duplicates are errors), the pending NeedToWrite
     declarations written after the attributes, newest first; an attribute whose namespace has no prefix in
     scope gets a generated `p{n}`; `xml:space` trimmed and checked; `xml:lang` passed through.
   - **`XmlEncodedRawTextWriter`**: declaration `<?xml version="1.0" encoding="utf-8"[ standalone="yes|no"]?>`
     (standalone from the part's declaration), nothing between it and the first node; text escapes `&` `<` `>`
     and CR as `&#xD;` (LF and TAB raw); attribute values `&` `<` `>` `"` and TAB/LF/CR as `&#x9;` `&#xA;`
     `&#xD;`; an empty element is `<a />` (space before the slash), an open one `<a></a>`; comments with
     `--`/trailing `-` split by a space; CDATA with `]]>` split; a character outside XML 1.0 (controls,
     U+FFFE/FFFF) is an `ArgumentException`.

### 1.4 `DocxDocument` (`api::document`)

`DocxDocument { pkg: Package, dom: Dom, xml: IndexMap<fold key, (name, document node)>, structure_changed,
main_part, limits, ids }`. `open` reads every entry once (`Package::verify`: the C# inflates all parts at load,
so a damaged entry or a limit fails the open), drops directory entries (the C# skips them), resolves the main
part (`officeDocument` relationship, else `word/document.xml`) and checks `w:document/w:body`. Part lookup is
the C# `FindPart` (exact, then `OrdinalIgnoreCase` with the .NET tables); the parsed-part map is keyed by the
`OrdinalIgnoreCase` fold, as the C# dictionary. Relationship targets resolve with the uo-opc D6 rule
(`part_name::resolve_relationship_target`, the port of the C# `OpcPartName`). `save` = normalize
(`DocxStructure`) and collect garbage (relationship ids searched in every attribute; parts unreachable from
`_rels/.rels`; orphan `.rels`) when anything changed, then serialize the dirty parts; `save_parts` returns the
same parts without the zip (the parity tests). `ids` replaces `RandomNumberGenerator` (paraId/sdt id on a
collision, every new `w:nsid`): xorshift64*, seeded per document from the process's random hash keys,
`set_seed` for tests.

### 1.5 Views

The C# reference types that hold `XElement`s become plain values holding `NodeId`s, used with the document
they came from (every method takes `&mut DocxDocument`, checks the ids): `CustomInline { ty, json,
part_path, sdt }`, `Shape { run }`, `RowTemplate { row (detached copy), table, appended_rows }`,
`BodyWriter { options, sect_pr, line, written, reuse, next_doc_pr_id }`, `SanitizeStage { inlines, … }`.
`ParagraphProperties` (C# `DocxParagraphProperties`, an opaque pPr copy) owns a small arena of its own, so a
sample extracted from one document can be applied to another, as in C#. In the facade each view is a
`readonly struct` of (document handle, id) or, for the stateful ones (writer, row template, stage), a handle
of its own (§4).

### 1.6 Host callbacks: two-phase (U13, G13)

- `DocxSanitizeOptions.IsCustomInlineValid` → `SanitizeOptions::validate_custom_inlines` +
  `sanitizer::sanitize_begin(doc, opts) -> SanitizeStage` (runs every step up to the validator and lists the
  inlines, `(type, json)` in document order, exactly what C# `Find(doc)` passes to the callback) and
  `SanitizeStage::finish(doc, verdicts)` (one verdict per inline, `Argument` otherwise; unwraps the false ones,
  then the remaining step, customXml). One-shot `sanitize` = all valid. The C# calls the callback per inline
  between unwraps, but `Find` materializes the list first and the callback cannot see the document, so the
  two forms are equivalent (measured: the `valid` script).
- `DocxCustomInlines.UnwrapInvalid(doc, Func)` → `custom_inlines::find` + `unwrap_where(doc, list, verdicts)`.
- `DocxImporter.ImportBody(Func<XElement, XElement>)` is internal (only `ApplyContent` with
  `PreserveSections`): a Rust closure, never crosses the ABI.
- The `Regex` overloads (`DocxText.Replace(doc, Regex, …)`, `ParagraphText.Find(Regex)`) are not ported
  (unused by the host; CHANGELOG entry for the coordinator: "Editing API (.NET): `DocxText.Replace(…, Regex,
  …)` removed"). The one regex the API uses itself, `#\{[A-Za-z0-9]+\}`, is a hand matcher.

## 2. The .NET-semantics table (U13)

Every row is measured by the harness (`semantics.capture`, site in brackets) or by the corpus scripts.

| .NET | Rust (`api::net` unless named) | Measured |
|---|---|---|
| `string` = UTF-16; indices, `Length`, `IndexOf` | positions in UTF-16 units (`units`, `index_of`); text kept as `String`, converted at the points that index | `ignorecase`, `wholeword`, corpus |
| `StringComparison.OrdinalIgnoreCase` (equality, `IndexOf`, `StartsWith`/`EndsWith`, `HashSet`/`Dictionary` comparers) | per-unit fold through the measured table `FOLD` (`net_tables.rs`, generated by `dotnet-harness tables`, invariant mode as the frozen suite) | `ignorecase` (1,029) |
| `ToUpperInvariant` (string and char) | per-unit `UPPER` table | `trim`, `firstword` |
| `char.IsLetterOrDigit` (whole word) | `LETTER_OR_DIGIT` ranges (a surrogate is never a letter) | `wholeword` |
| `char.IsWhiteSpace`, `Trim`, `TrimStart`, `IsNullOrWhiteSpace` | the 25-unit set, checked against the runtime by the generator | `trim` |
| `char.IsControl`, `Uri.IsHexDigit`, `char.IsAsciiDigit` | `is_control`, ASCII | generator |
| regex `\d` (Unicode Nd), `\s` (= IsWhiteSpace), `$` (end or before a final LF) | `is_decimal_digit_unit` (the BMP Nd blocks), `is_white_space` | `color`, `unique` |
| `int.TryParse(s)` / `(s, Integer, Invariant)` / `(span, None, Invariant)`, `long.TryParse` | `parse_int`, `parse_int_digits`, `parse_long`: ASCII digits, white `[\t-\r ]`, sign, trailing NULs, overflow = false | `ints` |
| `int.Parse` on a `\d` match (Colors) | `FormatException` for a non-ASCII digit | `color` |
| `double.Parse(num, Invariant)` of `[0-9.]+` (VML) | `FormatException` for two points or no digit, else `uo_jsnum::parse::to_number` (correctly rounded) | `vml` |
| `x.ToString("0.##", Invariant)` | 15 significant digits (`uo_jsnum` `to_exponential`), then half away from zero at 2 decimals, `-0` kept | `fmt`, `vml` |
| `Math.Round(double)` | half to even (`round_half_even`, the one allowed `round_ties_even`) | `fit`, `image` |
| `(long)double` | saturating, NaN → 0 (.NET 9+ = Rust `as`), named helper | `fit`, `image` |
| `float` arithmetic (`maxWidthPt * EmuPerPoint`) | `f32` | corpus `shapes` |
| `int` arithmetic (`max + 1`, `pos += 12 + length`, `space * (count - 1)`) | `wrapping_*` (unchecked C#) | `image`, corpus |
| `Math.Abs(int.MinValue)` | `OverflowException` (`ErrorKind::Overflow`), escapes `ImageInfo.TryRead` as in C# | `image` |
| `Uri.TryCreate(s, Absolute)` + `Scheme` | `absolute_uri_scheme`: a model of System.Uri for http/https/ws/ftp-like (two slashes `/` or `\`, authority ends at `/?#`, user part up to the first `@` without `\`, IPv6 literal, labels of ASCII alnum/`-`/`_` or any non-ASCII, no length limits, port ≤ 65535), mailto (leading `/` = path; else user@host or host; empty valid; empty host after `@` invalid), file (implicit DOS/UNC paths), news, generic | `uri` (4,329 inputs incl. default and wide scheme lists) |
| `Uri.UnescapeDataString` | `%XX` < 0x80 decoded; runs ≥ 0x80 decoded as UTF-8, each invalid maximal subpart kept as written | `unescape`, `tag` |
| `Uri.EscapeDataString` | unreserved kept, others `%XX` upper case | `escape`, `tagfmt` |
| `JsonDocument.Parse` (CustomInlineTag) | `json_valid`: RFC 8259, one value, depth 64, no comments/trailing commas; lone `\uD800` escapes accepted (as .NET) | `tag` (3,020), `tagfmt` |
| `DateTime.TryParse(…, Invariant, AdjustToUniversal \| AssumeUniversal)` | ISO 8601 date/date-time (fraction, `Z`, `±hh:mm`), `M/d/yyyy[ time]`, `d MMM yyyy`, RFC 1123 | `date` (565) |
| `DateTime.ToString("o")` / `"yyyy-MM-dd'T'HH:mm:ss'Z'"` | `UtcDateTime::round_trip` / `w3cdtf` (UTC only: the facade converts a local `DateTime`) | corpus `props` |
| `Path.GetExtension` | from the last `.` after the last `/` or `\`, `""` for a trailing dot | `unique` |
| exceptions | `ApiError { kind, message }`, `kind` = the C# class (`Argument`, `ArgumentOutOfRange`, `InvalidOperation`, `InvalidData`, `Xml`, `LimitExceeded{limit,max,part}`, `Format`, `Overflow`, `Json`); `class()` gives the C# name, `code()` the ABI code | corpus `err` |
| `null` | `Option`; `ArgumentNullException` cannot happen (Rust has no null), `ArgumentException.ThrowIfNullOrEmpty` → `Argument` for `""` | — |

## 3. Limits

`api::DocxLimits` = the C# `DocxLimits` (`max_part_bytes` 256 MiB, `max_total_bytes` 512 MiB, `max_xml_depth`
256, `max_fields_per_story` 100,000, `max_field_nesting` 64) plus the uo-opc caps the C# does not have
(`max_entries` 65,535, `max_xml_elements` 20,000,000; class `OfficeLimitExceededException` with Rust-only
limits `Entries`/`XmlElements`). Rust-only bounds where the C# recurses (§7 A6): style/numbering/note import
depth 256 (`InvalidOperation`), part-copy chains iterative (140,000 frames).

## 4. The C ABI the facade will need (design only; `uo-ffi` owns it)

- **Handles**: `UoDocx*` (a `Box<DocxDocument>`), `UoBodyWriter*`, `UoRowTemplate*`, `UoSanitizeStage*`,
  `UoParagraphProps*`; each with `uo_*_free`. C#: one `SafeHandle` subclass per handle
  (`ReleaseHandle` → free; finalizer path covered by the M4 test of `core/dotnet/UniversalOffice.Native.Tests`).
  Writer/row-template/stage handles keep the document's stamp; calling them with another document is the
  `InvalidOperation` code (§1.1), and the C# facade keeps a reference to the owning document object so the
  document handle outlives them.
- **Views without handles**: `CustomInline`, `Shape` are `readonly struct`s of (document handle, `NodeId`
  as two `u32`), plus the strings copied out at creation (`type`, `json`, `part_path`).
- **Calls**: `int32_t uo_docx_<op>(UoDocx*, in…, out…)` returning 0 or an error code (`ApiError::code`:
  1 Argument, 2 InvalidOperation, 3 InvalidData, 4 Xml, 5 LimitExceeded, 6 Format, 7 Overflow, 8 Json,
  9 ArgumentOutOfRange); details through `uo_last_error(UoDocx*, UoError*)` (`limit`, `max`, part name, UTF-8
  message), mapped by the facade to the same .NET exception types the C# threw (`OfficeLimitExceededException`
  with `Limit`/`Maximum`/`PartName`).
- **Data**: strings in as UTF-8 `(ptr, len)`, out as owned `UoBuf { ptr, len }` freed by `uo_buf_free`;
  lists out as a count + indexed getters (`uo_docx_inlines_count`, `uo_docx_inline_at`) — no callbacks, no
  pointers into Rust memory survive a call.
- **Validator**: `uo_docx_sanitize_begin(doc, opts*, &stage)`, `uo_stage_count`, `uo_stage_inline(stage, i,
  &type, &json)`, `uo_stage_finish(doc, stage, const uint8_t* verdicts, n)`; the facade keeps the old
  `Func<string,string,bool>` signature by running the two phases itself (the callback runs in managed code,
  so no exception crosses the boundary — the alternative `[UnmanagedCallersOnly]` design is not needed).
- **Panics**: none are possible by construction (no `unwrap`, no indexing, `forbid(unsafe_code)`), and every
  export still wraps its body in `catch_unwind` → code 2 with message "internal".
- **Threads**: a `UoDocx*` is `Send`, not `Sync`; the facade locks per document.

## 5. Parity harness

`crates/uo-docx/dotnet-harness/` (`UoD3Harness.csproj`, net10.0, invariant globalization as the frozen test
suite, project reference to the frozen `UniversalOffice.Docx`, the frozen test builders `DocxBuilder.cs` and
`SampleTemplate.cs` linked, not copied). Build output outside the repository
(`--artifacts-path core/target/agent-d3/dotnet`). Modes:

| Mode | Output | Content |
|---|---|---|
| `tables <out.rs>` | `src/api/net_tables.rs` (then `rustfmt`) | `IsLetterOrDigit`, `ToUpperInvariant`, `OrdinalIgnoreCase` fold of every BMP unit; `IsWhiteSpace`/`IsControl` checked against the Rust sets |
| `fixtures <dir>` | `tests/parity/d3/fixtures/*.docx` (35, ≤ 6 KB each, no fonts) | the C# Editing suite's cases as files: revisions, comments, sections, headers, fields (nested, simple, HYPERLINK variants, unterminated), hyperlinks, plugin inlines, customXml, OLE/controls/altChunk/subDoc, macros/protection/attached template, metadata, pictures/VML, text boxes, tables, styles/numbering/fonts, notes/bookmarks, odd XML (empty/open forms, comments, PIs, CDATA, char refs, default namespaces, non-BMP), depth at and over the limit, 70 open fields, 600 closed fields in one paragraph, prefix-less/BOM/UTF-16/case-folded names, the security probes |
| `semantics <out> <tags>` | `tests/parity/d3/semantics.capture` (18,068 records, 1 MB) | the U13 sites on generated inputs, internals reached by reflection |
| `corpus <out> <root> <list> [--dump dir]` | `tests/parity/d3/corpus.capture` (50,436 records, 6.4 MB) | 18 scripts × 2,802 documents (`corpus.list`: the fixtures + every tracked docx outside `core/`) |

Scripts (`Scripts.cs` ⇄ `tests/parity_d3.rs`, same order): `open`, `content`, `output`, `valid` (host
validator), `empty`, `apply` (sample markers), `applysec` (PreserveSections), `import`, `text` (plain text,
instructions, contains, replace case-insensitive with a non-BMP-free multi-unit replacement, parameters with
TAB/CRLF, replace with LF in the body, remove paragraphs), `fields`, `unsafe` (each step alone), `props`
(every getter, setters incl. control characters and empty values, a save in the middle), `writer` (every
format, links, images, bad alignment), `shapes`, `inlines` (replace/find/hyperlink/text/unwrap/again),
`parts` (`GetPartBytes` of every part incl. dirty ones, content types, relationships, `Clone`), `rowtpl`,
`samples`. Facets: `ret` (results joined by `|`; `=`text if short, else `#`digest), `pkg` (SHA-256 of every
saved part's name and decompressed bytes in package order — the zip bytes themselves differ by deflater and
are not compared), `err` (exception class; `OfficeLimitExceededException:<Limit>`). The import scripts mask the
values after `paraId="`, `nsid w:val="` and `<w:id w:val="` on both sides (C# `RandomNumberGenerator`).

Recapture (from the repository root, after `dotnet build … --artifacts-path core/target/agent-d3/dotnet`):
`H=core/target/agent-d3/dotnet/bin/UoD3Harness/release/UoD3Harness.exe`; `$H fixtures
core/crates/uo-docx/tests/parity/d3/fixtures`; `$H semantics core/crates/uo-docx/tests/parity/d3/semantics.capture
core/crates/uo-docx/tests/parity/custom-tag.inputs.txt`; `$H corpus core/crates/uo-docx/tests/parity/d3/corpus.capture
. core/crates/uo-docx/tests/parity/d3/corpus.list`. The corpus run takes ≈ 55 s; the Rust replay ≈ 6 min in
a debug build (`UO_D3_FILTER=<substring>` replays a subset, `UO_D3_DUMP=<dir>` writes the Rust parts of a
mismatching key for a diff against the harness's `--dump`).

## 6. Parity results

| Capture | Records | Equal | Accepted | Rounds |
|---|---|---|---|---|
| `semantics.capture` | 18,068 (22 sites) | 18,047 | 21 (A1) | 4 (test escape of non-BMP units; Uri: slashes/backslashes, mailto paths, first `@`, no length limits; UnescapeDataString/JSON 0; dates: three extra forms) |
| `corpus.capture` | 50,436 (2,802 documents × 18 scripts; 100,872 facets) | 100,872 | 0 | 0 |

The first full replay (2,801 documents, before the two fixture changes) gave 100,836 equal facets, 0
mismatches; after the fixes of the lint pass and the recapture with `deep-256/257` at the real boundary and
`sanitizer-gaps`, 100,872 equal, 0 mismatches (475 s, debug build). The scripts reserialize parts of nearly
every document (packages differing from the untouched open: `empty` 2,796, `apply`/`applysec` 2,791,
`import` 2,792, `props`/`writer` 2,796, `text` 786, `content` 488, `unsafe` 422, `shapes` 375, `output` 228,
`parts` 187, `samples` 174, `fields` 54, `inlines` 20), and the error classes match on every failing run
(1,396 `InvalidOperationException` of F1, `XmlException`, `InvalidDataException`, `FieldNesting`, `XmlDepth`).

Fuzzing (stable driver, `fuzz/src/d3.rs`, 150 s each after the seeds of the fixtures): `api_open_sanitize_save`
3,830,404 executions, `api_template_apply` 3,292,828, 0 crashes, slowest 79 ms. The first run's 2,330
"crashes" were all F2 (a sanitized save that no longer opens): the invariant now accepts `InvalidData`.

## 7. Accepted differences (by name, with evidence)

- **A1 — lone surrogates** in .NET strings (21 semantics inputs): a Rust `&str` cannot hold them; the API
  takes `&str`, so the facade's UTF-16 → UTF-8 conversion replaces them (`U+FFFD`) before the call. Parsed XML
  cannot produce them.
- **A2 — `OrdinalIgnoreCase`/`ToUpperInvariant` of supplementary characters** (Deseret, Adlam…): folded per
  BMP unit here, by code point in .NET. No case in the corpus or the semantics inputs.
- **A3 — `DateTime.TryParse` forms** beyond ISO 8601, `M/d/yyyy`, `d MMM yyyy`, RFC 1123 (the .NET parser
  takes many more): `None` here. 0 of 565 inputs differ; `dcterms` dates are W3CDTF.
- **A4 — `Uri` for schemes outside http/https/mailto** (ldap, news, file details, generic authorities): a
  model, not the parser; 0 of 4,329 inputs differ, but only the three default schemes are claimed exact.
- **A5 — encodings**: a part declaring an encoding other than UTF-8/UTF-16 (`windows-1252`, `iso-8859-1`) is
  read as UTF-8 (uo-opc), where .NET decodes it (Latin-1) or refuses it (unsupported code page); invalid UTF-8
  is replaced where .NET throws. None in the corpus.
- **A6 — recursion**: the C# importer recurses through `basedOn`/numbering/note references and part
  relationship chains and overflows its stack (a process crash) a few thousand levels down; the port stops at
  depth 256 with `InvalidOperation` and copies part chains iteratively (tested: 300-deep chain, 2,000-part
  chain).
- **A7 — zip bytes and part order**: the saved archive's compressed bytes differ (miniz_oxide vs .NET's zlib);
  parts are compared decompressed. `Package::save` writes integer-like names first (the TS `zipSync` object
  order); the C# keeps insertion order — no such names in any corpus.
- **A8 — the host's random ids** (`paraId`/`sdt` id on collision, `nsid`): a seeded generator instead of
  `RandomNumberGenerator`; masked in the parity capture.

## 8. Memory (measured, `tests/memory_d3.rs`)

| Gate | Result |
|---|---|
| M1 | 145 documents (fixtures + every 25th corpus document) × 12 passes (each open → op → save → drop): 0 B left each; pure functions and error paths (open errors, limit, foreign ids): 0 B. Costliest pass allocated 113 MB (`many-fields.docx`, 600 fields in one paragraph, before the `following_elements` fix) |
| M2 | 1,000 × (open content → sanitize(Content) → save → open template → apply_content → save): live bytes flat at 91,104 B over 21 samples, back to the start after `clear_caches` |
| M3 | generated bodies of 100/1,000/10,000 paragraphs (≈ 20 KB/200 KB/2 MB of XML): open allocates 35×/28×/23× and peaks at 13×/14×/12× the XML; open + sanitize + save allocates 76×/50×/49× and peaks at 27×/14×/12× (budgets asserted: allocated ≤ 120×, peak ≤ 60×). (the C# `GC.GetAllocatedBytesForCurrentThread` reference was not measured) |
| M4 | the facade's handles (design §4); the test lives with `uo-ffi`/`core/dotnet` |
| M5 | no cache outlives a document (the arena and the package's inflated parts go with it); the arena is bounded by the package caps plus the work done |

## 9. Security review of the sanitizer

The sanitizer (`DocxSanitizer`) is the host's security boundary. The port keeps the C# behaviour exactly
(parity); what follows are the findings of the review of that behaviour, for the maintainer. None is fixed
in the port (C# frozen; a fix changes results and needs a decision).

| # | Finding | Severity | Evidence / recommendation |
|---|---|---|---|
| S1 | **A hyperlink relationship without `TargetMode="External"` is never checked against the scheme allowlist**: `RemoveUnsafeContent` only drops *external* relationships with a disallowed scheme; an internal one whose `Target` is `javascript:alert(1)` survives `Content` sanitization with its `w:hyperlink`. Word resolves it as a part (dead link), but a consumer that reads hyperlink targets as URLs (HTML export, the TS reader, other tools) sees `javascript:`. | medium | fixture `sanitizer-gaps.docx` (`content`/`output` scripts keep `rId90`). Recommend: check every relationship of type hyperlink by its target scheme, whatever its mode |
| S2 | **Package-level relationships (`_rels/.rels`) are not examined** by `RemoveUnsafeContent` (it walks the parts, and the package is not a part): any relationship there — external or of an unsafe type — survives. | low | `sanitizer-gaps.docx` (`rId91`). Recommend: include the source `""` in the walk |
| S3 | **VML `href` attributes** (`<v:shape href="javascript:…">`, Word's legacy shape hyperlinks) are not relationships and are not touched. | low–medium | `sanitizer-gaps.docx`. Recommend: drop or scheme-check `href` (and `o:href`) on VML elements |
| S4 | **Non-story parts are not scanned for active elements or fields**: the glossary document (`word/glossary/document.xml`, building blocks), comments (removed only by relationship) — `w:object`, `w:control`, `w:altChunk`, `DDEAUTO` fields stay in the glossary part (their relationships are dropped by the part walk, so the objects lose their targets). | low | `sanitizer-gaps.docx`. Recommend: include the glossary part in `Stories` for the sanitizer, or remove it |
| S5 | **Unterminated complex fields are ignored**: a `begin` without `end` (any instruction, e.g. `DDEAUTO`) is neither replaced nor removed; its `w:instrText` stays. Word does not run a field without end, so the risk is low; a lenient consumer could. | low | `open-fields.docx`, the field scan (`fields::scan`). Recommend: drop the `instrText` of fields that never close |
| S6 | **Stack exhaustion in the C# importer** (`ApplyContent`/`ImportBody` on untrusted content): `ImportStyle` ↔ `ImportNum`/`ImportAbstract` ↔ `ImportNote` and `CopyPart` recurse once per chained style, list, note or related part; a crafted content document with a few thousand chained `basedOn` (or part relationships) overflows the .NET stack, which terminates the server process (not catchable). | **high for the C# host** | fixed by name in the port (A6); the C# stays as is until 0.10 ships the Rust path. Recommend: if the C# 0.9 line gets a patch, bound the depth there too |
| S7 | **Quadratic work on untrusted text**: `ParagraphText.Find` restarts `IndexOf` one unit after each rejected match (O(n·m) per call, n calls in the worst case); `ReplaceTextWithImage` recomputes `NextDocPrId` (a walk of every story) for each match. Both are linear in practice (short needles, trusted templates); with an adversarial template they are not. | low | port keeps them. Recommend: a work budget if templates can come from users |
| S8 | **Encodings**: the Rust reader decodes a part declared `windows-1252` as UTF-8 where .NET decodes Latin-1 (A5): a host validator written in C# and the Rust sanitizer could read different text from the same bytes. The PDF path reads the sanitized bytes, so "checked ≠ opened" does not reach the output; noted for G27. | low | A5 |

Two C# defects outside security, found by the harness and the fuzzer and kept for parity (decisions for the
maintainer):

- **F1** — `DocxText.Replace` with a replacement containing TAB/CR/LF fails with `InvalidOperationException`
  ("The parent is missing") when two matches fall in the same `w:t`: the matches are applied back to front on
  segments built once, and the first replacement detaches the `w:t` (`ReplaceWith` of the run content) that
  the next one still points to. 1,396 of the 2,801 corpus documents hit it in the `text` script (replacing
  `a` by `b` LF `c`); the document is left half-replaced.
- **F2** — a document whose main part is found through the `word/document.xml` fallback (no `officeDocument`
  relationship in `_rels/.rels`) loses it at the first save after a change: `CollectGarbage` keeps only what is
  reachable from the package relationships, and deletes the main part (and everything below it). The saved
  package no longer opens (`InvalidDataException`). Fixture `no-main.docx`; found by `api_open_sanitize_save`.

Checked and fine: DTD prohibited and no entity expansion (uo-opc strict), depth/element/part/total caps
(verified at open), zip bombs (inflated bytes counted), relationship targets through the one D6 rule (the
part checked is the part opened), external relationships other than allowed hyperlinks removed, OLE/ActiveX/
altChunk/subDoc/frames removed with their relationships and unreachable parts collected at save, macros and
their relationships removed and the content type reset, attached template and mail-merge sources removed,
fields outside the allowlist replaced by their cached result (nested codes removed with them), HYPERLINK
fields converted only for allowed absolute URLs or internal anchors, customXml removed, the host validator's
list complete and in order (two-phase), no panic on any input (fuzz + no-panic tests), stale/foreign node
ids refused.

## 10. Files touched outside `src/api/`

- `crates/uo-docx/src/lib.rs`: `#[cfg(feature = "editing-api")] pub mod api;` (one line).
- `crates/uo-docx/dotnet-harness/**` (new), `crates/uo-docx/tests/parity/d3/**` (new),
  `crates/uo-docx/tests/{parity_d3,parity_d3_semantics,memory_d3,nopanic_d3}.rs` (new).
- `core/fuzz/src/d3.rs` (new), `core/fuzz/src/lib.rs` (the `d3` module and two `TARGETS` entries),
  `core/fuzz/Cargo.toml` (`editing-api` in the uo-docx features).
- `core/docs/modules/uo-docx.md`: a pointer to this record under "D3 — Editing API".
- The `editing-api` feature line already existed in `crates/uo-docx/Cargo.toml` (no change).

## 11. Process metrics

| Stage | Who | Wall time (approx.) | Rounds / rework |
|---|---|---|---|
| Reading (skill, plan rows, the 26 C# files, uo-opc strict parser) | Opus | ≈ 35 min | — |
| C# harness (5 files) and the three captures | Opus | ≈ 30 min | 2 harness fixes (frozen helpers that throw: `Colors` `FormatException` on an Arabic-Indic digit, `ImageInfo` `OverflowException` — both real C# behaviour, now captured as errors) |
| DOM, reader, serializer, document (architecture) | Opus | ≈ 60 min | first compile: 4 errors |
| API translation (19 modules) | Opus (the plan's Sonnet share) | ≈ 90 min | compile: 2 borrow errors |
| Semantics parity | Opus | ≈ 20 min | 4 rounds (above) |
| Corpus parity | Opus | ≈ 10 min + the replay | 0 rounds |
| Memory, no-panic, fuzz, lint gates, record | Opus | ≈ 60 min | M3 finding fixed (`following_elements`) |

## 12. What is left

- The `uo-ffi` exports and the C# facade (design §4) — owned by the bindings work; the M4 handle test with it.
- The maintainer's decisions on S1–S8 (port and C# keep the current behaviour).
- G27 (ship the hybrid NuGet or wait for the Rust PDF path) — unchanged.
- CHANGELOG entry for the removed `Regex` overloads (§1.6), for the coordinator.
