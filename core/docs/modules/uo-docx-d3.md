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

## 4. The C ABI and the C# facade of the Editing API (built; `uo-ffi` owns the ABI)

Built in milestone 2 (frontier D3-ABI): `core/crates/uo-ffi/src/editing/` (the C ABI: 92 exports — 86 with a status plus the 6 handle frees of `free_fn!` —, 1,940 lines with its
unit tests) and `core/dotnet/UniversalOffice.Docx/{Editing,Native,Core}` (the facade, 2,389 lines), over the Rust
Editing API of §1 (feature `editing-api`, enabled by `uo-ffi/Cargo.toml`). Every behaviour comes from Rust; the ABI
only translates, the facade only adds the .NET types and the lock.

### 4.1 Conventions (module doc of `uo-ffi/src/editing.rs`)

- **Status**: every export returns an `int32` (`ApiError::code`: 1 Argument, 2 InvalidOperation, 3 InvalidData,
  4 Xml, 5 LimitExceeded, 6 Format, 7 Overflow, 8 Json, 9 ArgumentOutOfRange; 12 Internal is the caught panic of the
  shared `pipeline::code` space). The `*_free`s, `uo_docx_native_bytes`, `uo_sample_set_count` and the error getters
  return no status. Bodies run in `error::api` / `error::with_doc`, which clear both error slots and call the
  milestone-1 `guard` (`catch_unwind`).
- **Error details**: the shared per-thread slot of `pipeline` (`uo_last_error`, `uo_last_error_message`, 24 bytes,
  shared with wasm: G2) carries code, maximum and the English message; a second per-thread slot of
  `editing/error.rs` carries what that struct cannot: the `OfficeLimit` ordinal and the part name
  (`uo_last_error_limit` gives `UoLimitInfo{limit, part_len}`, `uo_last_error_part` the bytes).
  `uo_editing_error_clear` clears BOTH and drops their strings (the facade calls it after reading, so M4 sees 0 B).
  Limit ordinals follow the C# `OfficeLimit` (PartBytes 0, TotalBytes 1, XmlDepth 2, FieldsPerStory 3,
  FieldNesting 4) plus the Rust-only Entries 5 and XmlElements 6.
- **Strings and lists**: UTF-8 `(ptr, len)`; `len == usize::MAX` is null (never the pointer: an empty .NET array may
  pin to a null pointer). Lists and records go in and out as ONE packed little-endian format (`editing/wire.rs`
  and `Native/Wire.cs`): a string is `u32 len` (`0xFFFF_FFFF` = null) plus UTF-8; a list is `u32 count` plus the
  items; a record list is `u32 count` plus the fields of each record in the order the export documents. A count
  larger than the bytes left is refused before any allocation (`Reader::count`); truncated or garbage input is the
  Argument code, never a panic. Owned results are `UoBuf`s (`uo_buf_free`); an optional result leaves the slot null
  (the C# `null`).
- **Option structs** are `#[repr(C)]`, start with `size: u32` (shorter = Argument; longer from a newer facade is
  accepted) and carry `Str{ptr,len}` fields and flag bits. `UoParagraphFormat` carries a `*const UoParagraphProps`
  for `Base`. Every export that takes a pointer is `pub unsafe extern "C" fn` with a `# Safety` section (as the
  milestone-1 exports of `lib.rs`): the caller promises that each `(ptr, len)` is null or readable for `len` bytes
  during the call, each handle is null or live and used by one thread at a time, each output slot is null or
  writable. `editing/wire.rs` holds the helpers that turn pointers into references; they are `unsafe fn` with their own
  `# Safety`, and each export wraps its body in ONE `unsafe` block with the single SAFETY line that forwards the
  contract (`unsafe_op_in_unsafe_fn` and `undocumented_unsafe_blocks` are deny). The exports with no pointer
  (`uo_editing_error_clear`) stay safe. (An earlier draft kept the exports safe and hid the dereference in `pub`
  helpers to pass clippy's `not_unsafe_ptr_arg_deref`; that was unsound, since any safe Rust caller could pass a
  dangling pointer, and was reverted in fix round 1.) The unit tests wrap their calls in `unsafe` blocks; Miri
  (`--lib`) is clean.
- **Threads**: `UoDocx` is `Send` (a compile-time assertion for every handle), used by one thread at a time; the
  facade takes a per-document lock on EVERY call path (writer, row template, shapes, inlines included).
- **Panics**: `with_doc` catches a panic inside the body, sets `poisoned` and answers INTERNAL; every later call on
  that handle is `InvalidOperation` ("poisoned by an earlier internal error"). Tested in `editing/tests.rs`.

### 4.2 The exports (92 after fix round 2: 91 `unsafe extern "C"`, plus the pointer-free `uo_editing_error_clear`; `pure.rs` has 10, the two `uo_opc_*` included; the facade imports all 92, name sets diffed equal at merge)

| Family (file) | Exports |
|---|---|
| document (`document.rs`, 17) | `uo_docx_open(data,len,*UoDocxLimits,out)`, `_save`, `_clone`, `_native_bytes`, `_main_part_path`, `_part_names`, `_has_part`, `_get_part_bytes`, `_get_relationships`, `_get_relationship`, `_get_content_type`, `_prop_get` / `_prop_set` (which: 0 Title … 9 ApplicationName), `_prop_get_date` / `_prop_set_date` (`UoDateTime`), `_custom_property_names`, `_set_custom_property` |
| operations (`ops.rs`, 20) | `uo_docx_run(op)` (0 Template.Empty, 1 AcceptAllRevisions, 2 RemoveComments, 3 DisableTrackChanges, 4 RemoveProtection, 5 RemoveAttachedTemplate, 6 RemoveMacros, 7 RemoveHeadersAndFooters, 8 MergeSections, 9 RemoveCustomXml, 10 RemoveCustomProperties, 11 RemovePersonalMetadata); `_sanitize`, `_sanitize_begin`, `uo_stage_inlines`, `uo_stage_finish`, `_remove_unsafe_content`; `_fields_replace_disallowed`, `_fields_convert_hyperlinks`, `_fields_instructions`; `_text_replace`, `_text_replace_parameter`, `_text_replace_parameters`, `_text_contains`, `_text_remove_paragraphs_containing`, `_text_plain`; `_template_apply_content`, `_template_import_body`; `_numbering_add_restarted`, `_pictures_fit_to_page`, `_samples_contains` |
| views (`views.rs`, 20) | `uo_docx_shapes_find_containing`, `_remove_containing`, `_replace_text_with_image`, `_first_in`, `_clear_header_footer`; `uo_shape_text`, `_clear_text`, `_set_fill_color`, `_set_fill_transparent`, `_set_text_color`, `_remove`; `uo_docx_inlines_find`, `_replace_text`, `_unwrap_where`; `uo_inline_title`, `_text`, `_is_attached`, `_replace_with_text`, `_replace_with_hyperlink`, `_unwrap` |
| writer (`writer.rs`, 16) | `uo_body_writer_new`, `_line_is_empty`, `_add_text`, `_add_break`, `_add_image`, `_end_paragraph`, `_finish`; `uo_row_template_take`, `_append`, `_appended_rows`; `uo_docx_samples_extract`, `uo_sample_set_count`, `_name`, `_take`; `uo_paragraph_props_style_id`, `_num_id` |
| pure (`pure.rs`, 8) | `uo_tag_try_parse`, `uo_tag_format`, `uo_fields_is_allowed_url`, `uo_fields_try_parse_hyperlink`, `uo_pictures_fit`, `uo_image_info_try_read` (`UoImageInfo`), `uo_editing_defaults`, `uo_docx_paragraph_style_name` |
| errors (`error.rs`, 3) | `uo_last_error_limit`, `uo_last_error_part`, `uo_editing_error_clear` |
| frees (`handles.rs`, 6 + the milestone-1 `uo_buf_free`) | `uo_docx_free`, `uo_body_writer_free`, `uo_row_template_free`, `uo_stage_free`, `uo_sample_set_free`, `uo_paragraph_props_free` |

Handles: `UoDocx`, `UoBodyWriter`, `UoRowTemplate`, `UoSanitizeStage`, `UoSampleSet`, `UoParagraphProps`; none holds a
pointer to another (every call that needs the document receives it), so any free order is safe. The `*_free`s are
plain `Box` drops and touch no thread-local (they run on the finalizer thread).

### 4.3 Deviations from the original design, each with its reason

1. **Per-thread error slot plus a detail slot** instead of `uo_last_error(UoDocx*, …)`: G2 fixed the 24-byte
   `UoErrorInfo` shared with wasm; the limit and the part name need a slot of their own (D3A-1).
2. **Views are tagged indices into per-document tables** (two `IndexSet<NodeId>` in `UoDocx`, one for shape runs and
   one for inline sdts), not raw `NodeId` bits: the fields of `NodeId` are `pub(crate)` and `uo-docx` is not this
   frontier's. A view value is `kind (1 bit) | document salt (8 bits) | index (23 bits)`; the salt comes from a global
   counter (wraps at 256). What is detected, and tested (`editing_abi.rs`): a forged index past the table, a view of the
   other kind (an inline view given to a `uo_shape_*` export and the reverse: exact), and a view of ANOTHER document
   even when that document also holds views (exact unless the two salts collide: two documents opened a multiple of 256
   documents apart). What is NOT detected: a `stale` view (the tables never shrink, so a node that was unwrapped or
   removed keeps its index; the Rust API then answers about a detached node as it does for the C#), and a salt collision.
   The C# facade cannot produce any of these mixes (each `DocxShape` / `DocxCustomInline` carries its own document and
   kind); the guard is for raw C callers. A document that hands out more than 8,388,607 distinct views of one kind
   answers `InvalidOperation`. Cost: the table keeps every distinct node ever handed out until the document is freed
   (bounded by the arena, as detached nodes are); a clone starts with empty tables and its own salt (D3A-3).
3. **Packed lists in one `UoBuf`** instead of count plus indexed getters: fewer crossings and no handle per list;
   `'\n'`-joined lists (the milestone-1 convention) cannot be used because parameter values contain line breaks
   (D3A-4).
4. **`DocxShape` / `DocxCustomInline` are sealed classes**, not readonly structs: `FirstShapeIn` returns
   `DocxShape?` in hosts (D3A-3).
5. **`uo_docx_run(op)`** serves the 12 no-argument operations with stable op numbers (D3A-13).
6. **Poisoned handles** after a caught panic (D3A-12); **same-document `ImportBody`** imports from a clone, never two
   `&mut` to one box (D3A-10); **count saturation**: counts are `i32::try_from(n).unwrap_or(i32::MAX)` (D3A-11).
7. **No `Create(...)` returning an `XElement`** in `DocxCustomInlines`; `DocxPageArea.From(XElement?)` is a verbatim
   managed port and `DocxStyles.ParagraphStyleName(doc, XElement)` reads the `w:pStyle` in managed code and resolves
   it in Rust by style id (D3A-2; `uo_docx_paragraph_style_name` allocates no node: it calls the additive
   `styles::paragraph_style_name_of_id` of `uo-docx`, which shares its tail with `paragraph_style_name`).
8. **Exceptions**: the 0.9 `UniversalOffice.Core.OfficeLimitExceededException {Limit, Maximum, PartName}` now lives in
   the UniversalOffice.Docx assembly (`Core/OfficeLimits.cs`, with `OfficeLimit` and `OpcPackageLimits`), derives
   from `UniversalOfficeException` and adds `Stage`; the milestone-1 `UniversalOffice.OfficeLimitExceededException`
   (never published) is gone and the PDF path throws the Core one with `Limit = Conversion` (D3A-1, D3A-9).
9. **Additive change outside the owned files**: `uo-ffi/Cargo.toml` enables `uo-docx/editing-api` (one word);
   `uo-ffi/src/lib.rs` gets `pub mod editing;` and one doc line.

### 4.4 The C# facade (`UniversalOffice.Docx`, net8.0, no NuGet dependencies)

`DocxDocument` is `IDisposable`: it holds a `DocxHandle` (SafeHandle; Dispose and the finalizer both end in
`uo_docx_free`), a per-document lock taken by every call, a monotonic `Order` so that `ImportBody(a, b)` locks the two
documents in a fixed order, and `GC.AddMemoryPressure(uo_docx_native_bytes)`, removed on Dispose or by its finalizer
(D3A-7). `DocxBodyWriter`, `DocxTableRowTemplate` and `DocxParagraphProperties` are `IDisposable` too (additive).
The two-phase validator runs inside `DocxSanitizer.Sanitize`: `sanitize_begin`, then `uo_stage_inlines`, then the
host's `Func<string,string,bool>` for each inline, then `uo_stage_finish`, all under a `using` of the stage, so an
exception from the host callback propagates and frees the stage (D3A-6). Errors map in `Native/EditingErrors.cs`:
1 `ArgumentException`, 2 `InvalidOperationException`, 3 `InvalidDataException`, 4 `System.Xml.XmlException`,
5 `OfficeLimitExceededException{Limit,Maximum,PartName}`, 6 `FormatException`, 7 `OverflowException`, 8
`System.Text.Json.JsonException` (D3A-5), 9 `ArgumentOutOfRangeException`; null arguments are
`ArgumentNullException` in managed code, as in 0.9. The default lists (`DocxFields.Default*`) are read from Rust once.
`dotnet/UoNative.targets` builds `uo-ffi` once (`--features count`, release) for the four .NET test and tool projects
(`-p:UoSkipCargo=true` copies a library cargo already built). Projects: `UniversalOffice.Docx.Tests` (facade, Editing
contract, errors by class, memory, threads), `UniversalOffice.Docx.FrozenSuite` (the frozen C# Editing suite),
`UniversalOffice.Docx.Parity` (the D3 differential harness through the facade), `UniversalOffice.Docx.Smoke`
(`edit <in.docx> <template.docx> <out.docx>`, the clickCompliance flow, 202 ms on the fixtures).

### 4.5 Public API differences against the published 0.9 (`UniversalOffice.Docx` 0.9.3), for the in-scope types

Measured by `api-dump` (`dotnet run --project core/dotnet/UniversalOffice.Docx.Parity -- api-dump <0.9
UniversalOffice.Docx.dll>`): the public types of `UniversalOffice.Docx.Editing`, `CustomInlineTag` and the four Core
types the Editing API throws, takes or that a host validator calls (`OfficeLimit`, `OfficeLimitExceededException`,
`OpcPackageLimits`, `OpcPartName`), member by member (36 types in the frozen assembly, 36 in the facade, 11
differences). `OpcPartName` is a PUBLIC static class of the 0.9 `UniversalOffice.Core`, and clickCompliance calls
`ResolveRelationshipTarget` in production code (`DocxPackageValidationService`) and in its contract tests: the facade
provides it (`ResolveRelationshipTarget` and `EscapeSegment`, both over `uo_opc_resolve_relationship_target` /
`uo_opc_escape_segment`, the ONE rule of `uo_opc::part_name`, so the host validator and the core cannot diverge), with
no difference in signature. OUT of scope, printed by `api-dump` by namespace: the other 380 public types of the two
frozen assemblies (the C# document model, layout reader, PDF writer, exporters, DrawingML and Fonts; Core: `DmlColorMods`,
`JsNumber`, `MiniXml`, `OpcPackage`, `Units`, `XmlElement`); the facade does not provide them and clickCompliance does
not reference them (checked by search in its source: only `UniversalOffice.Docx.Editing`, `CustomInlineTag`, the
limits types and `OpcPartName`).

| # | Difference | Kind |
|---|---|---|
| 1 | `OfficeLimit` gains `Entries`, `XmlElements` (Rust-only caps) and `Conversion` (PDF-path limits) | 3 added members |
| 2 | `OfficeLimitExceededException` is not `sealed` any more and derives `UniversalOfficeException` instead of `Exception` (adds `Stage`, `Code`); the public constructor `(OfficeLimit, long, string?)` is kept | changed base (2 lines) |
| 3 | `DocxDocument`, `DocxBodyWriter`, `DocxTableRowTemplate`, `DocxParagraphProperties` get `Dispose()` (`IDisposable`) | 4 added |
| 4 | `DocxCustomInlines.Create(string, string, XElement?)` returning an `XElement` is removed (unused by clickCompliance and by the frozen tests; it would need an rPr fragment parser in Rust) | removed (D3A-2) |
| 5 | `DocxText.Replace(doc, Regex, …)` is removed (§1.6) | removed |

Semantic differences that no signature shows: exception **messages are the Rust core's, in English** (0.9's were
Portuguese; classes and properties are equal and nothing matches on messages, D3A-8); bad JSON throws the public
`JsonException` where 0.9 threw the internal `JsonReaderException` (its subclass, D3A-5); a throwing host validator
leaves NO inline unwrapped (0.9 had unwrapped the ones before it; the document is mid-sanitization in both, D3A-6);
documents are thread-safe (one call at a time), disposable and report GC memory pressure (D3A-7); a negative cap in
`DocxLimits` / `OpcPackageLimits` counts as 0 (D3A-11: 0.9 made every part exceed, empty ones included; now empty
parts pass); counts above `int.MaxValue` saturate; `null` items inside string collections are
`ArgumentNullException`; a lone surrogate in a string argument becomes U+FFFD on the way to Rust (A1, as in the
milestone-1 UTF-8 writer); the `UniversalOffice.Core` types now live in the UniversalOffice.Docx assembly, so a host
that also references the 0.9 `UniversalOffice.Core` package (through Xlsx or Pptx) gets CS0433 (clickCompliance
references only UniversalOffice.Docx and is unaffected, D3A-9); operations on a shape removed from the document
behave as on 0.9's detached `XElement` (no throw).

### 4.6 What was verified (every run with `CARGO_TARGET_DIR=core/target/m2-d3abi`, `-j 4`)

- **Rust, function level** (`cargo test -p uo-ffi`): lib 11 passed (wire round trips, null and short structs, every
  `*_free(null)`, the poisoned handle, the limit detail of `deep-257`); `editing_abi` 11 passed (all 35 d3 fixtures:
  30 open and save byte-equal to `DocxDocument::open(..).save()`, 5 refused with codes 3 to 5; each error class and
  code; `deep-257` gives limit 2, max 256, part `word/document.xml`; the two-phase sanitizer equals the Rust
  two-phase byte for byte and a second finish is InvalidOperation; forged and foreign views on the 11 view exports;
  same-document import; a writer of another document is InvalidOperation); `editing_nopanic` 2 passed in 1.6 s
  (40 random rounds over 9 documents through every export that takes a document, string, list, date or option struct
  (plain getters of a handle are left to `editing_abi`, listed in the test doc), with null and garbage pointers,
  lengths, option bits and dates, truncated lists and forged views, `apply_content` with garbage and with each hostile
  document as the content and `import_body` with it as the source; 60 hostile, truncated or bit-flipped documents,
  also through `apply_content` and `import_body`: no INTERNAL).
- **Memory, Rust** (`editing_memory`, counting allocator, 49 s in debug): M4 10,000 × each handle kind (UoDocx,
  UoBodyWriter, UoRowTemplate, UoSanitizeStage, UoSampleSet with UoParagraphProps, UoBuf) and 10,000 × (limit error
  and xml error): **0 B left**; M1 every export family over 7 fixtures: 0 B left each; M2 soak of 200 × (open,
  sanitize two-phase, save, apply_content into a template, sanitize, save): live series constant at 7,717 B over 21
  samples (`UO_FFI_EDITING_SOAK=1000` for the 1,000-iteration run).
- **.NET, `dotnet test UniversalOffice.Docx.Tests`** (net8.0): **43 passed of 43** = the 14 milestone-1 tests (68 of
  68 PDF parity, M4 converters, thread soak) plus 29 new Editing tests (contract 10, errors 10, memory 6, threads 3).
  M4: 10,000 documents Open, Save, Dispose **0 B left**; 10,000 abandoned and finalized **0 B left**; 10,000 ×
  (document, writer, row template, sample set, paragraph props, sanitize stage) 0 B; 20,000 failed opens 0 B;
  10 rounds × 35 fixtures through sanitize, text, fields, inlines, save 0 B. Threads: 8 threads × 50 flows equal to
  the sequential bytes with native memory flat; 8 threads × 20 rounds on ONE document without a crash and flat;
  Dispose racing 4 readers is safe. The 29 Editing tests take 8 s.
- **The frozen C# Editing suite through the facade** (`UniversalOffice.Docx.FrozenSuite`, the 12 frozen files
  linked): **134 passed of 134** (11 s). Two tests cannot run over Rust and are cut from a generated copy in
  `obj/` (the frozen sources are untouched; a pattern that stops matching fails the build):
  `DocxBodyWriterTests.Resultado_Passa_Pela_Leitura_Do_Motor_De_Layout` (reads the result with the C# layout
  reader) and `DocxTextTests.Expressao_Regular_Apaga_Parametros_Que_Sobraram` (the removed Regex overload).
  `DocxUntrustedInputTests` is linked whole since fix round 2: its `OpcPartName.ResolveRelationshipTarget` assertion
  (the escape `%2531`) runs against the facade's `OpcPartName` (round 1 had cut it, on the false ground that the
  type was internal); one helper
  (`OpcPackageLimitsTests.LieAboutSize`) is a verbatim copy in `FrozenShims.cs`. `DocxUntrustedInputTests` asserts
  `Limit`, `Maximum` and `PartName` exactly as 0.9 threw them.
- **Parity harness through the facade** (`UniversalOffice.Docx.Parity`, net10.0): `corpus-check` gives **records
  50,436, facets equal 100,872 of 100,872, mismatches 0** in 36 s (the C# took about 55 s), the same result as the
  Rust API replay of `tests/parity_d3.rs`; `semantics-check` checked 13,429 public or pure-.NET sites: 0 mismatches,
  26 accepted (10 lone-surrogate inputs, A1; 16 `JsonReaderException` seen as `JsonException`, D3A-5), and skipped
  4,639 reflection-site records (builtin, color, date, firstword, nextrelid, relative, removeinvalid, unique, vml,
  wholeword: they reach C# internals). Both re-run in fix round 2 on the final tree (corpus 36.4 s; semantics 13,429
  checked, 26 accepted, 0 mismatches, 4,639 skipped).
- **Lint**: `cargo clippy -p uo-ffi --all-targets` with no features, with `--features count`, and the check-changed
  gate (`--all-features`, native and wasm32): clean; `tests/float-casts.sh` and `tests/grep-bans.sh` clean; rustfmt
  clean on every touched file. All C# builds with 0 warnings.
- **Size** (release, LTO, feature `count`, same target dir, fix round 1): `uo_ffi.dll` is **8,589,824 B with** the Editing
  API and **7,496,704 B without** it (`editing-api` removed from `uo-ffi/Cargo.toml` and `pub mod editing;` commented
  out, nothing else), a difference of 1,093,120 B (+14.6 %).
- **Fix round 2** (reviewer findings): (1) `OpcPartName` (public in 0.9, called by clickCompliance) provided by the
  facade over two new pure exports, `uo_opc_resolve_relationship_target` and `uo_opc_escape_segment` (92 exports in all, counting the 6 frees),
  with a call-shape test in `EditingContractTests` and the FrozenSuite cut removed; `api-dump` scope widened to it and
  prints the out-of-scope set (§4.5). (2) `uo_docx_template_import_body` catches a panic around `import_body` and
  poisons the SOURCE handle as well as the target (D3A-12). (3) `editing_nopanic` now also drives `text_plain`,
  `fields_instructions`, `custom_property_names`, `main_part_path`, `pictures_fit_to_page`, `prop_set_date` /
  `prop_get_date` with garbage fields, `shapes_first_in`, `apply_content`, `import_body`, `tag_format`,
  `is_allowed_url`, `try_parse_hyperlink`, the two OPC exports, `editing_defaults` and `open`; the doc lists the plain
  getters it leaves to `editing_abi`. (4) `uo-docx` `api/styles.rs`: `paragraph_style_name` now delegates to
  `paragraph_style_name_of_id` (one tail), with the test `nopanic_d3::paragraph_style_name_of_id_equals_the_node_door`
  over every paragraph of the d3 fixtures (files outside this frontier, additive; declared for the orchestrator).
  (5) The in-tree `bin/` and `obj/` of the .NET projects were removed; every run passes `--artifacts-path
  target/m2-d3abi/dotnet`. Re-run: `dotnet test UniversalOffice.Docx.Tests` 43/43 (32 s), FrozenSuite 134/134,
  `cargo test -p uo-ffi` `editing_abi` 11, `editing_nopanic` 2, `editing_memory` 4; `nopanic_d3` 4; clippy
  `-D warnings` clean for uo-ffi (no features, `count`) and uo-ffi + uo-docx `--all-features`, and uo-docx wasm32;
  `float-casts.sh` and `grep-bans.sh` clean. `node tools/check-changed.mjs` stops at `uo-schema` (a clippy
  `collapsible_match` in `core/tools/uo-schema/src/lib.rs`, a file of the WS frontier, not touched here), so the
  clippy steps were run per crate instead.
- **Merge (orchestrator, the third review's findings)**: the two `PatchFrozen` patterns end in `\r?\n` (they
  matched LF only, so a CRLF checkout — `core.autocrlf=true`, the windows-latest runner — failed the FrozenSuite
  build; probed on LF and CRLF copies of both frozen files: both match, same cut); the generated sources are listed
  by name instead of `$(PatchDir)*.cs` (a stale round-1 copy broke the Debug build with CS0111); `uo_stage_finish`
  reads `verdicts` before it takes the stage, and `uo_tag_try_parse` checks both output slots before writing any,
  so a failing call leaves no stage consumed and no buffer the caller was not told about; export counts corrected
  (92). The third review re-ran `corpus-check` (100,872 facets equal), `semantics-check` (0 mismatches) and
  `api-dump` (11 differences, §4.5).
- **Fix round 1** (reviewer findings): the exports are `unsafe extern "C"` (§4.1); view tables split by kind with a salt
  (§4.3 item 2); `uo_docx_view_count` removed (it was a test-only export with no C# import); `uo_docx_paragraph_style_name`
  no longer allocates arena nodes; the `editing_nopanic` sample-set leak is freed. Re-run after them:
  `cargo clippy -p uo-ffi --all-targets -- -D warnings` clean with no features and with `--features count`;
  `cargo clippy -p uo-docx --all-targets --all-features` clean native and for wasm32; Miri
  (`cargo +nightly miri test -p uo-ffi --lib editing`) 5 passed, 2 ignored; `cargo test -p uo-ffi --features count --lib
  --test editing_abi --test editing_nopanic --test editing_memory --test ffi_memory --test hostile_inputs`: all green
  (editing_abi 11 tests incl. the new view-mix test; editing_memory, the 10,000-handle M4 test, 4 passed in 49 s);
  `dotnet test UniversalOffice.Docx.Tests -c Release` (cargo release build inside, 1 min 10 s) **42/42**;
  `dotnet test UniversalOffice.Docx.FrozenSuite` **134/134**; `corpus-check` through the facade **50,436 records,
  100,872 of 100,872 facets equal, 0 mismatches** (34.9 s).

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

- **Decisions of this frontier waiting for the maintainer** (the variants below are implemented; pending decisions
  D2a-U6, D2a-C2, D2a-C1, G23-G31 and G27 keep the variants already implemented): D3A-1 one
  `UniversalOffice.Core.OfficeLimitExceededException` (not sealed, with `Stage`); D3A-2 keep `DocxPageArea.From` and
  `DocxStyles.ParagraphStyleName` over `XElement`, drop `DocxCustomInlines.Create`; D3A-3 views as sealed classes over
  a per-document index table; D3A-4 packed lists; D3A-5 public `JsonException` for the internal `JsonReaderException`;
  D3A-6 two-phase validator semantics (an exception from the host callback leaves no inline unwrapped); D3A-7
  `IDisposable`, GC memory pressure and a per-document lock; D3A-8 English exception messages (a CHANGELOG line);
  D3A-9 the Core types inside the UniversalOffice.Docx assembly (a 0.10 `UniversalOffice.Core` package is the
  alternative, needed when Xlsx and Pptx move to Rust); D3A-10 `ImportBody(doc, doc)` imports from a clone (not
  measured against 0.9); D3A-11 negative caps count as 0, counts saturate; D3A-12 a caught panic poisons the handle;
  D3A-13 `uo_docx_run(op)`.
- The maintainer's decisions on S1-S8 (port and C# keep the current behaviour) and G27 (ship the hybrid NuGet or wait
  for the Rust PDF path): unchanged.
- **For the milestone-2 gate**:
  1. (done in fix round 1) `dotnet test UniversalOffice.Docx.Tests` with the cargo release build inside (42/42), the
     FrozenSuite (134/134) and `corpus-check` (0 mismatches) ran on the compiling tree; `semantics-check` was re-run in fix round 2 (§4.6).
  2. (done) The size of `uo_ffi.dll` with and without `editing-api`: 8,589,824 B and 7,496,704 B (§4.6).
  3. (done locally, in fix round 1 — before the round-2 edits of `pure.rs` and `import_body`) Miri `--lib` over the editing unit tests: 5 passed, 2 ignored (the parse-heavy ones); the CI job
     must still run it.
  4. A `ffi_editing` fuzz target in `core/fuzz` (not owned here: proposed) and LSan over the facade (G19).
  5. The CI `dotnet` job has never run (the edit adds `10.0.x` to setup-dotnet, the FrozenSuite, both parity checks and
     the Smoke `edit`); the first run on Linux also exercises `libuo_ffi.so` through the shared `UoNative.targets`.
  6. (fixed in round 1) `uo_docx_paragraph_style_name` allocated up to three arena nodes per call; it now resolves the
     style id directly (`styles::paragraph_style_name_of_id`, additive in `uo-docx/src/api/styles.rs`: a file outside
     the owned list, declared for the orchestrator). GC pressure of an open document is still measured once, at open.
- **For the orchestrator, outside the owned files**: the CHANGELOG (.NET) entries (English messages, `IDisposable`
  documents, `Create` and the `Regex` overloads removed, the Core types in the UniversalOffice.Docx assembly, the
  exception hierarchy); the §8 M4 row of this record should point at the facade numbers of §4.6;
  `uo-bindings.md` should cross-reference §4; `uo-ffi/Cargo.toml` (one word, `editing-api`) and `uo-ffi/src/lib.rs`
  (`pub mod editing;`, one doc line) and the additive `styles::paragraph_style_name_of_id` in `uo-docx/src/api/styles.rs` are the only edits outside the owned list.
- The CHANGELOG entry for the removed `Regex` overloads (§1.6), for the coordinator: unchanged.

### Process metrics (this frontier)

| Stage | Wall time (approx.) | Rounds / rework |
|---|---|---|
| Reading (record §4, the 12 Rust API modules, the 0.9 C# signatures, the milestone-1 ABI and facade) | 25 min | 0 |
| Rust ABI (wire, errors, handles, options, 6 export files) | 30 min | 2: clippy `not_unsafe_ptr_arg_deref` on the first design; the first answer (safe exports over `pub` helpers that hid the dereference) bypassed the lint and was unsound, so the review sent it back: exports and `wire` helpers are now `unsafe` with `# Safety` sections (fix round 1, ≈ 40 min incl. the test wrapping) |
| Rust tests (abi, memory, nopanic, lib unit) | 25 min | 3 test fixes (fixtures with shapes do not exist: a synthetic VML package; one output check moved before the work; `deep-257` setup) |
| C# facade (22 files) | 30 min | 0 compile rounds after the cell edits (pointer-in-lambda: 3 sites rewritten before the first build) |
| .NET tests (4 files) | 25 min | 2: 4 test expectations that were wrong (default content type of an unknown extension, detached shapes do not throw, an unbounded `ImportBody` growth in the thread test) |
| FrozenSuite | 20 min | 1: 7 compile errors from 3 frozen files, solved by the generated-copy patch (a first attempt wrote to a doubled absolute path) |
| Parity project, api-dump | 25 min | 2: `JsonException` rule (D3A-5), api-dump scope (first run listed the 190 C# model types) |
| Record, lint, CI | 20 min | 0 |
