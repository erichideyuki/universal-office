# Changelog

Changes per version of the published UniversalOffice packages (npm
`@universal-office/*` and NuGet `UniversalOffice.*`). How to publish:
`docs/publishing.md`.

## Unreleased — 0.10 (Rust core)

0.10 replaces the TS engines and the .NET port with one Rust core (`core/`), module
by module; the behaviour changes decided for each module are listed here. The crates
are not published: their APIs are internal until the npm, wasm and NuGet APIs of
0.10 are designed.

### Container layer (`uo-opc`: zip, deflate, XML, OPC packages)

- Sheet: an unclosed comment, CDATA section, processing instruction or tag at the
  end of an XML part is an error; the 0.9 parser looped forever on it.
- `&constructor;` and the other names of JavaScript object members stay literal
  text in XML, like any named entity other than the five predefined ones (Sheet and
  Slides replaced them with the JS member, as in `function Object() { [native code] }`).
- An entry whose deflate stream is empty but declares a size is corrupt (0.9 read
  it as that many zero bytes: a 132-byte file could make a reader allocate 256 MiB
  for a part).
- A part parsed as XML may hold at most 20,000,000 elements (about 10 times the
  largest real part, `giant-1m.xlsx`): one hostile 256 MiB part could otherwise
  become several GB of tree. The XML part cap (256 MiB) applies to every part
  parsed as XML, whatever its name. The nesting limit of 1,000 levels, which Write
  had, now applies to Sheet and Slides parts too (decided: no real part is deeper
  than 25 levels; the 0.9 parsers overflowed the JS stack past about 8,000).
- Entries after one named `__proto__` are kept (0.9 dropped those whose names are
  numbers that are not valid indices, such as `-1` or `1.5`).
- Saving copies an untouched entry as is only when its data really is what the zip
  declares (size, CRC-32, range inside the file); otherwise it is written again from
  the bytes read (deflated, or stored if it was stored), and an entry that cannot be
  read fails the save. An entry whose data overlaps another entry's is always read
  and written again. A stored entry that is copied keeps its method (0.9 deflated
  every entry again).
- In the browser (wasm32) a package may hold at most 512 MiB per binary part and
  1 GiB in total (2 GiB and 4 GiB natively), so the limit error comes before an
  allocation failure.
- Saved zips carry a fixed timestamp (2000-01-01 00:00) instead of the time of the
  save, so the same package saves to the same bytes.
- Saved zips use zip64 records when they need them (more than 65,535 entries, or a
  size or offset of 4 GiB), which fflate, the zip writer of 0.9, does not write.
- Relationship targets are resolved by one rule in Write, Sheet and Slides: the OPC
  rule of the .NET `OpcPartName` (relative to the source part, `.` and `..`
  segments resolved, `%xx` unescaped, the `#fragment` dropped) with part names
  compared ignoring ASCII case. 0.9 had one rule per app: Write joined `word/` to
  the target without resolving `..` and looked the name up exactly, so a Word
  document's `../customXml/item1.xml` (the sample `demo.docx`) was not found;
  Sheet and Slides only resolved leading `../`, and Slides looked names up
  exactly. No part that 0.9 found is lost.
- Sheet compares part names ignoring ASCII case only, the OPC rule; 0.9 used the
  full Unicode lowercase, under which `K` (U+212A KELVIN SIGN) named a part `k`.
  No real file is affected.
- Saves deflate at level 4 by default instead of 6: saving a 31.7 MB sheet takes
  160 ms instead of 790 ms, for an archive 0.8% larger (0.13–0.17% over ordinary
  documents). The contents of the parts are the same. An explicit level is the
  zlib level of the Rust compressor (miniz_oxide), 0–9.
- Internal API (`uo-opc`, not published): `Package::xml_part` is the one way to parse
  a part as XML, and `Package::resolve_target` the one way to resolve a relationship
  target (the OPC rule of the .NET `OpcPartName`, ASCII case folding);
  `Limits::bounded` gives the parse options of a package's limits, and the
  `.rels`/`[Content_Types].xml`/raw XML edit helpers take the package's `Limits`;
  `Package::entry` exposes the declared method, sizes and CRC of an entry for host
  validators; `Package::take_part` hands a part's bytes over without a copy (an edit
  of a large part holds it twice instead of three times); XML errors from
  `xml_part` name the part; `ContentTypes::content_type_of_part` and
  `override_of_part` follow the OPC name rule (the Slides exact rule is
  `content_type_of_exact`); `CaseFold::Unicode` is gone (`None` or `Ascii`);
  `zip::DEFAULT_LEVEL` is the default save level; `part_name::safe_relative_path`
  checks a part name before a host extracts it to disk; `xml::parse` takes the text
  by value (`String` or `Arc<str>`, kept by the tree without a copy).

### Write layout (Rust engine): rules measured in Word where 0.9 differed

Found by comparing with the WordCraft engine and measured in Word 365 for Windows
(pt-BR); each rule is in `core/docs/modules/uo-docx-layout.md` ("WordCraft study —
lines / page / tables / fields") with its cases. The 0.9 behaviour stays behind a
`LayoutOptions::ts_*` switch (wasm layout flag bits 18–21), used only by the parity
tests. The .NET port and the 0.9 TS engine keep the 0.9 rules.

- Lines: spaces of the space-like set give no line height; Arial's space after a list
  number; the jc=both compression tolerance; kinsoku per East Asian language with
  hanging punctuation; the character grid (linesAndChars, snapToChars) and the
  `*Chars` indents; doNotExpandShiftReturn; `w:ptab` relative to margin or indent; jc=
  distribute spreads over character gaps; a default tab left of the margin; bar tabs
  do not move the default stops; emphasis marks (`w:em`) are drawn and grow the line;
  superscript/subscript sizes above 24 pt per font.
- Page: no space before a paragraph that opens with a page break (compat 15); odd/even
  section starts with blank pages as Word; gutter, rtlGutter, gutterAtTop, mirror
  margins, book fold and two-on-one printing; inside/outside floats by the page's
  document parity; a float sits on the page and column of its anchor line, and Word's
  re-layout around it; mirrorIndents.
- Tables: a row kept with the next by its first cell's first paragraph; conditional
  table-style borders by region (insideH/insideV); floating-table wrap edges; wrapping
  objects clamped into the cell; a word that does not fit beside a float (mode 15 goes
  below it); a `w:tblStyle` after other `w:tblPr` children discards those before it.
- Fields and fonts: `\*` format switches on PAGE/NUMPAGES/SECTIONPAGES/PAGEREF/SEQ and
  formulas with the number words of pt-BR, en-US and es-ES; STYLEREF per page in
  headers/footers, by the style's UI name; SECTIONPAGES on shared pages; REF `\r`
  relative and `\p` per language; Word's formula grammar and error texts (header/footer
  and table formulas); list words per paragraph-mark language and `w:isLgl`; Symbol
  and Wingdings line metrics when missing and multiple spacing by the tallest run;
  PostScript, camel-case and full font names, charset-suffix names ("Arial CE",
  "Times New Roman Cyr"), a missing symbol-charset font, Thai and East Asian fallbacks.
- English hyphenation: CC0 patterns plus the Moby Hyphenator II exceptions (public
  domain) give Word's points on 95.8% of the measured paragraphs (hyph-utf8: 88.3%).
  The data is a new pattern set `en-us-word` (593,877 B raw, 220,531 B gzip), fetched
  instead of `en-us`; notices in `core/licenses`.

## Unreleased

### Document viewer: engine switch (`engine="rust"`)

- New attribute `engine` on `<universal-document-viewer>` (and the viewer entry of
  `@universal-office/write`): `ts` (default, the 0.9 engine, unchanged) or `rust`, the
  Rust core compiled to WebAssembly. With `rust` the document is laid out by the Rust
  engine, painted by the same canvas code from the pages it encodes, and the text
  layer, find, comments, headings, link boxes, print and "Save as PDF" come from it. On
  all 74 committed fixture documents (579 pages) both engines give the same page count and sizes,
  headings, link boxes, text layer, find hits, canvas pixels (byte for byte) and
  uncompressed PDF. Changing the attribute reloads the open document. The editor
  `<universal-write>` has its own switch (next entry).
- The `.wasm` ships as `chunks/uo-write.wasm` and is fetched on first use only: a viewer
  without `engine="rust"` never requests it. `UniversalDocumentViewerElement.configureEngine({
  wasm, js, engine })` points to another source (bytes, URL, `Response`, or the wasm2js
  build); one module per page, so the first load wins.
- A strict `script-src` needs `'wasm-unsafe-eval'` for `engine="rust"` (Chrome/Edge 97,
  Firefox 102, Safari/iOS 16.0; Safari 15 needs `'unsafe-eval'`). Hosts that cannot allow
  it keep `ts` or use the wasm2js build. See the viewer README, "Content Security Policy".
- Not yet in `rust`: plugin inline renderers are not painted and `udv-inline-click` is not
  fired; `getLinks()`, `getCustomInlines()` and `goToBookmark(name)` read the document with
  the TS reader on first use; `udv-link-click` `detail.text` is the text of the link box, not
  of the whole run when the run wraps. Closing or moving the element frees the engine's
  memory; reconnecting reopens the document (the scroll position resets).
- `rust` follows Word where `ts` does not (measured in Word): a PAGEREF field (a table of
  contents' page numbers, a cross-reference to a page) in a section with chapter page
  numbering shows the chapter number and its separator before the page (`1-i`), where `ts`
  shows the page alone (`i`); the editor also writes it so in the TOC entries it builds.

### Editor: engine switch (`engine="rust"`, preview)

- New attribute `engine` on `<universal-write>`: `ts` (default, the 0.9 engine,
  unchanged and fetching nothing) or `rust`, the editing session of the Rust core compiled
  to WebAssembly. With `rust` the document model, the editing commands, undo, the
  incremental layout and the page painting come from the engine; the element keeps the
  input, the bars, scrolling and zoom. The attribute is read at initialization.
- What `rust` takes: every public method (511) and every bar command (462) of the `ts` editor is
  routed to the engine or the element's shell; nothing is grayed out. It is not yet 1:1 with `ts` everywhere (the
  open items are listed below). That covers opening
  `.docx`/`.doc`/`.odt`, saving `.docx` (with or without fonts), `.odt`, PDF, HTML, Markdown and XPS, Print,
  Compare, Mail Merge, Word Count; typing and IME, the keyboard and mouse selection; the Ruler, Gridlines, Bookmark
  marks and the Read, Web, Draft and Outline views; character formatting (the Font dialog, colors, text effects,
  OpenType features, the Format Painter), paragraph formatting (the Paragraph, Tabs and Borders dialogs, hyphenation)
  and styles; lists (libraries, new bullets and numbers, multilevel lists, list styles, numbering values and indents);
  the clipboard (keyboard, buttons, Paste Special and Paste Link, the ring, the Paste Options, the paste-cells
  options); tables (Quick Tables, table styles, Draw Table, an embedded spreadsheet), headers and footers, notes, text
  boxes, pictures and shapes as objects (wrap points, transparent color, the text box gallery), page setup, sections,
  page design (color, watermark, page borders), the cover/watermark/header/footer/TOC galleries, plugin custom inlines,
  the context menu, comments and tracked changes, Find and Replace, fields, table of contents, captions,
  cross-references, bookmarks and links, and spelling with the shared Hunspell. Checked against `ts` over the 74
  committed fixture documents with 12 scripted steps of real input each, plus one harness per family; every remaining
  difference is declared in `core/docs/modules/uo-bindings.md`. Where `ts` and Word differ, the engine follows Word
  (measured; each case is declared there).
- The `.wasm` ships as `chunks/uo-write-editor.wasm` (the session build, about 10.4 MB, 3.5 MB
  gzip) and is fetched only with `engine="rust"`. `UniversalWriteElement.configureEngine({
  wasm, engine })` points to another source and `UniversalWriteElement.engineStats()`
  reports memory and live handles. One engine module per page: the editor's module also
  serves a `<universal-document-viewer engine="rust">` of the package loaded after it; a viewer
  engine loaded first makes `engine="rust"` on the editor fail with an error naming the
  editor module. `build:pkg` refuses to ship a test build (exports `test_`/`selftest_`/
  `memtest_`/`fixture_`, the counting allocator) as either module.
- Compare (`@universal-office/compare`): `loadPair(aBytes, bBytes, opts)` takes an optional
  `opts.diff(aBytes, bBytes)` that answers the `DocDiff` from another engine; the documents are still
  read by the element for the view. The editor's File > Compare under `engine="rust"` passes the Rust
  port of `diffDocuments` (the session module gains about the compare code's size).
- A strict `script-src` needs `'wasm-unsafe-eval'` (Safari 15: `'unsafe-eval'`); see the
  viewer README, "Content Security Policy".
- Nothing stays disabled in `rust`. A command or method added later without a route is shown grayed out and
  does nothing (it never runs on the TS model). Declared differences a host can see: events fire after the mutation;
  call `load*()` after `uw-ready`; reconnecting reopens the document without undo history; XPS is written from the
  engine's pages (no text, shape or picture effects, pattern fills or EMF/WMF/SVG pictures); `ts` and `rust` editors
  on one page do not share the Office Clipboard. Not yet 1:1 with `ts` (open, none disabled): the Web, Draft and
  Outline views are one sheet per section, not the `ts` continuous canvas; no live text reflow while a wrap point is
  dragged and no drop caret while an inline picture is dragged (the result at the drop is the same); the gray field
  shading outside the body (headers, footers, notes) and the frame of a text box edited inside another box are not
  painted; a gallery preview can show a cross-reference's page from the previous layout right after an edit; whether
  `ts` also keeps the `Table Grid` style after a save between a table insert and its undo has not been compared.

### Slides editor: engine switch (`engine="rust"`, preview)

- New attribute `engine` on `<universal-slides>`: `ts` (default, unchanged, fetching nothing) or `rust`. With `rust` the
  package, every editing method, undo/redo and `savePptx()` (font embedding included) run in the Rust editing session, in
  the same WebAssembly instance as the viewer's Rust engine, which paints the slides, thumbnails, slide show and playback
  from the edited package. The module is `chunks/uo-slides-editor.wasm` (it contains the viewer's engine too);
  `UniversalSlidesElement.configureEngine({ wasm, ppt })` sets the source of both elements (one module per page).
- Same results as `ts`, checked in Chromium step by step on scripted sessions (the package, the UI state, the slide and
  main-canvas pixels, the saved file). A command that fails throws and leaves the presentation exactly as it was (the `ts`
  editor could keep part of a multi-part change in the package without showing it). Not yet in `rust`: the selection, hit
  tests, the bars' state, find and the accessible text read a TS copy of the package that the engine keeps in step.

### Slides viewer: engine switch (`engine="rust"`)

- `<universal-slides-viewer engine="rust">` opens the deck in the Rust engine only (no TS model): the element instantiates
  `chunks/uo-slides.wasm` when it connects and `src` waits for it, so `loadPptx` opens synchronously in Rust. Slides,
  thumbnails, exports, the accessible text, hyperlinks, pictures, media, embedded fonts, `present()` and `play()` all come
  from the engine; pixels, accessible text, link hit-tests and media requests are equal to `ts` (Chromium). Opening costs
  about 40% less memory and 34% less time than the 0.9 hybrid. A `loadPptx` before the module is ready (or a `.ppt`) opens
  with the TS model first, which is dropped when the engine has the deck.
- Behaviour differences with `rust`: shape-level hyperlinks (`hlinkClick` on the shape) and slide/show jumps
  (`ppaction://hlinksldjump`, `hlinkshowjump`) are followed (the TS viewer only opened run addresses); `play()` returns a
  timeline whose `slides` carry the times of each slide (index, start, end, animation start, transition), not the TS placed
  effects; a picture whose embedded EMF+ bitmap is still decoding leaves the slide unpainted for that moment (the TS painted
  the rest of the slide). Metafile, PICT and TIFF pictures are painted by the engine (before, `engine="rust"` left them out).
- The Slides PDF (`uo_pptx::pdf`, .NET `UniversalOffice.Pptx`) writes hyperlinks as link annotations over the drawn text, and
  shape clicks and slide jumps as links (internal jumps go to the page).
- A font that arrives while a deck is open (from the network, or a bold/italic face fetched on first use) no longer leaves
  `engine="rust"` text laid out with the fallback face's widths and drawn with the real one (seen in real Safari 26 and iOS 26).
  Safari applies a face added to `document.fonts` to canvas text only at a later rendering update, with no event. For 1 s after a
  font change the engine now measures at every paint, as the TS engine does, and both engines measure again and repaint two
  frames later and when that window ends. The engine measures on a DOM canvas (before: an `OffscreenCanvas` when available).
  A face that takes effect later than that window is also caught now (seen on iOS 26: a WordArt kept the fallback's ink box,
  since Courier New and Liberation Mono share their advances). Before every paint the engine measures a probe text in each
  font of the layout on the canvas it paints. When the probe's width or ink box changed, the engine measures again.

### Safari 15.0–16.3

- Every product failed to load in Safari before 16.4 (macOS Safari 15.6, iOS 15.5; also in 0.9.2): the chart engine's word
  wrap used a regex lookbehind, which those versions reject when they parse the script, so the whole bundle failed. The split
  is now written without it (same words, proven over 300,000 random strings). The spell checker's address filter (`/usr/bin`,
  `./a/b`) had a lookbehind too, built when its module loads: on those versions spell checking would not start.
- Slides (both engines): path gradients and picture colour effects (duotone, grayscale, brightness/contrast) could come out
  one shade darker in Safari 15, depending on how long the page had been running. Once a loop is optimized, Safari 15's JIT
  truncates a fractional value stored into canvas pixel data instead of rounding it (91.7 became 91). The rasters now round
  in code (round half to even, the rule the store follows everywhere else) and store whole numbers. Other browsers paint
  exactly as before.
- Slides viewer `engine="rust"`: when the browser refuses a canvas to measure text (on iOS 15 that happens once the page's
  canvas memory is used up), the engine now measures on the canvas it paints, as the TS engine does, instead of failing
  the paint. Its measuring canvas is now 1 x 1 px.

### Opening documents on slower machines

- `<universal-write>` lays the document out far fewer times while it opens
  (15 → 5 for the site's sample): background re-layouts (a font arriving from the
  network, hyphenation patterns, fonts the layout asked for) now wait 80 ms after
  the last request (300 ms at most after the first) instead of one full layout per
  arrival, and any direct `refresh()` covers a pending one.
- The fonts embedded in a .docx are measured before the first layout: the text is
  laid out with them from the start (no second layout when they arrive) and the
  browser `FontFace` only triggers a repaint. This also fixes text laid out with the
  fallback metrics and painted with the real font (uneven spacing in a title).
  A font the browser rejects is removed and the layout redone, as before.
- `SfntFont` keeps the GPOS kerning it read per font file (the same bytes were
  parsed again for every registration — 71 reads for 19 fonts).
- Measured with the CPU slowed down 4× (a mid-range machine): time until the
  editor is interactive 11.2 s → 5.6 s, main thread blocked 7.7 s → 2.6 s, 3.7 MB
  downloaded instead of 5.3 MB, JS heap 32 → 21 MB.

## 0.9.3 — 2026-10-31

All npm and NuGet packages ship as 0.9.3 (the products depend on
`@universal-office/font-files` 0.9.3). The desktop app's first public release is
0.9.3 (the 0.9.2 desktop build stayed a draft).

### Phones (compact bar)

- Below 600px the compact bar of `<universal-write>`, `<universal-sheet>` and
  `<universal-slides>` becomes a phone bar, like Google's mobile apps: the
  `leading` item, the title, the `trailing` actions and "⋮" (the menus) on one
  line; trailing pills with an icon show only the icon. The bar root gets the
  class `uo-narrow`; `COMPACT_NARROW_WIDTH` (600) is exported.
- New `BarContext.dock`: in the phone bar the toolbar row moves into that
  element (`<universal-write>` passes its bottom bar, so the formatting tools sit
  at the bottom of the screen, like Google Docs). The row gets `uo-docked`.
- `<universal-write>` on a phone (the host gets the class `uw-narrow`, the status
  bar is hidden): like Google Docs, it opens in **Web Layout** at 100%, the text
  reflowing to the screen width with 16px margins (the continuous views keep
  0.42" margins on wider screens); **Print Layout** is a choice in View, with the
  page zoomed to the screen width. The view and the zoom are kept per side of the
  breakpoint: crossing it (a resize, turning the phone) restores the other side's.
  New `pageWidthZoom()`: the zoom at which the page fills the visible width
  (Word's "Page width"). `setView()` now also syncs the bars.
- Touch selection in `<universal-write>`: a long press selects the word under the
  finger without opening the virtual keyboard, and dragging without lifting the
  finger extends the selection (the page does not scroll meanwhile). A text
  selection made with the finger shows two **selection handles** below its start and
  end, like Google Docs on a phone (a canvas has no native ones): dragging one moves
  that end of the selection. While a handle (or the finger after the long press)
  is dragged near the top or bottom of the visible area, the document scrolls by
  itself, faster the closer to the edge; the drag moves once per frame, repaints
  only the pages between the old and the new end, and syncs the bars on release.
  Above the selection (below it when there is no room
  under the top bar) a floating **action bar** offers Cut, Copy, Paste and Select
  all, as the native one does over DOM text: it hides while a handle is dragged,
  the selection is extended or the page scrolls, comes back when the finger
  lifts, and closes after an action until the next selection. Cut and Paste are
  left out of read-only documents. There is no context menu on touch — browsers
  do not let a page add its items to the native one, and a menu of our own over
  the text got in the way; formatting is in "⋮" and in the toolbar. On Android the native long-press
  `contextmenu` becomes this selection, and the click on release no longer moves
  the caret. Pages have no system callout or loupe on long press.
- Toolbar snackbar (`.uo-toast`): the text color now reads the theme's
  `--u-color-on-inverse-surface` (it used a token that does not exist and fell
  back to near-white, unreadable on the light inverse surface of dark themes).
- Menus on narrow screens: a submenu opens in place of its parent, with a
  "← <parent>" row on top, instead of a cascade that does not fit. On touch
  screens menu items are 44px tall and keyboard shortcuts are not shown.
- Menus, the collapsed groups' popovers and the galleries follow their button when
  the window changes only in height (the virtual keyboard, the browser's own bars on
  phones) or the page scrolls, instead of closing — on phones the bottom toolbar's
  menus opened and closed at once. A width change, or the button leaving the screen,
  still closes them.

### Fixes

- `<universal-write>`, `<universal-sheet>` and `<universal-slides>` contain their
  z-index (`isolation: isolate` on the host): the sticky bars no longer paint over
  a sticky header of the page when the component scrolls under it.
- The editors' menus no longer stay fixed on screen when the page scrolls: they follow
  their button (and a context menu, which has none, closes).
- The keyboard shortcuts of `<universal-write>`, `<universal-sheet>` and
  `<universal-slides>` no longer throw on a `keydown` without `code` (synthetic
  events, some automation drivers).

## 0.9.2 — 2026-10-30

### Versions and packages

- **New:** `@universal-office/converter` (npm) and `UniversalOffice.Converter`
  (NuGet), first publication: DOCX, XLSX, PPTX and PDF to HTML, PDF or Markdown,
  in the browser, in Node.js and in .NET. It is the converter the editors already
  used for their exports (the `@tools-universal/office-converter` submodule, never
  published), now in this repository (`converter/`), with its OOXML processors
  bundled in.
- All packages ship as 0.9.2, including the ones that were at 0.0.1: npm
  `write`, `document-viewer`, `compare`, `sheet`, `slides`, `slides-viewer`,
  `font-files`, `spellcheck`, `dictionary-pt-br`, `dictionary-en-us` and
  `dictionary-es`; NuGet `UniversalOffice.Core`, `.Docx`, `.Xlsx` and `.Pptx`
  (`.Docx` depends on `.Core` 0.9.2). First publication of `sheet`,
  `slides`, `slides-viewer` and `dictionary-es`.
- **Renamed:** the Slides viewer now carries the product name, like the
  `<universal-slides>` editor. The `@universal-office/presentation-viewer` package
  became `@universal-office/slides-viewer`, the
  `<universal-presentation-viewer>` element became `<universal-slides-viewer>` and the
  `UniversalPresentationViewerElement` class became
  `UniversalSlidesViewerElement` (also re-exported by
  `@universal-office/slides`, whose `UniversalSlidesElement` extends it).
  There is no alias for the old name: change the import, the tag and the CSS selector. The
  events (`us-*`), the attributes, the `::part()`s and the CSS variables
  (`--upv-*`) do not change. `@universal-office/presentation-viewer` was
  published only as 0.0.1 and is deprecated on npm, pointing to the new one
  (`npm deprecate @universal-office/presentation-viewer "Renamed to
  @universal-office/slides-viewer"`). Write keeps
  `@universal-office/document-viewer` / `<universal-document-viewer>`.

### .NET

- Fonts on Linux: `JsPath.Join` (port of Node's `path.join` in
  `UniversalOffice.Core`) joined paths with `\` outside Windows as well,
  so no system font folder was found, the system font index
  was empty and PDF generation failed with "nenhuma
  fonte disponível" (no font available) (not even Liberation Serif was found). Outside Windows it
  now joins with the OS separator; on Windows the behavior (parity with
  the JS) does not change.

### Desktop app

- The three desktop apps (Universal Write, Universal Sheet and Universal Slides,
  each with its own Electron) became ONE app, **Universal Office (Beta)**
  0.9.2 (`desktop/`, appId `org.universaloffice.UniversalOffice`): one
  installer and one runtime for the three editors, one window per document, and
  a file opened by the system (association, 2nd instance, macOS
  open-file) goes to the editor for its extension. Everything the three apps did still works: single
  instance, recent files and Jump List (now per editor), native Open/Save/Save
  As/Export, printing through the system dialog, confirmation of
  unsaved changes on close (including the Slides note not yet
  saved), frameless window with the OS controls in the theme color, navigation
  and IPC guards, OS fonts, offline Write spell checking.
  The preload bridges keep each editor's names (`wordnext`,
  `universalSheet`, `universalSlides`).
- Per-editor welcome screen (like the JetBrains IDEs'): the editor's shortcut
  (`--editor=write|sheet|slides`) opens its welcome window — icon,
  name and version, New document/spreadsheet/presentation, Open…, the recent files
  OF that editor (search, open with click/Enter, remove, clear; a file that
  disappeared shows as unavailable), shortcuts to the other two, language and
  third-party licenses. Without an argument: the welcome screen of the last editor used.
- Interface language (Portuguese, English, Spanish) on the welcome screen and in a
  Language menu in each editor (through the component's plugin API), stored
  in the profile; default = the spell-checking language, from the system. It applies
  immediately, without reloading: app texts (dialogs, notices, Jump List) and the
  editors' `lang`.
- Icons: the REAL ones of each app (`site/assets/icons`), rasterized by
  Electron's Chromium (the SVGs use CSS that librsvg does not render), in every
  place — windows and taskbar, shortcuts, file types,
  Linux/Flatpak launchers, welcome screen.
- License notices for everything the app bundles in `resources/licenses`
  (Hunspell, pt-BR/en-US/es dictionaries, hyphenation, fonts, libraries),
  opened by "Licenças de terceiros" (Third-party licenses).
- Snap with four apps (the generic one and Write/Sheet/Slides, each with its own
  launcher); Windows installer in pt-BR/en-US/es depending on the system.
- Windows: each editor with its own AppUserModelID, icon and taskbar
  group (`setAppDetails`); the installer creates the "Universal Write
  (Beta)", "Universal Sheet (Beta)" and "Universal Slides (Beta)" shortcuts in the Start
  menu and on the desktop, with a single entry in Apps.
- Size: the Windows installer is ~106 MB, versus ~322 MB for the three
  separate installers — a single Chromium, the open fonts only once and
  only the interface languages (en-US, pt-BR, es, es-419).
- Linux: one AppImage/deb/rpm/tar.gz/snap `universal-office`; the deb and the rpm
  install the three launchers, and so does the Flatpak
  (`org.universaloffice.UniversalOffice`, replacing the three
  manifests). Release by the `v<version>` tag; the
  `write-v*`, `sheet-v*` and `slides-v*` tags and the `*/apps/electron` folders are gone.

### Sheet

- `<universal-sheet>` got its own bars, like those of `<universal-write>`
  and on the same framework (`@universal-office/ui`), wired through the same
  attributes: `topbar` (compact bar, the default — title above the menus
  "Arquivo · Editar · Exibir · Inserir · Formatar · Ferramentas · Ajuda" (File · Edit · View · Insert · Format · Tools · Help) and a
  tools row that collapses by priority — or `toolbar="ribbon"`,
  with the tabs "Página Inicial · Inserir · Fórmulas · Exibir · Ajuda" (Home · Insert · Formulas · View · Help)), a formula
  bar with Name Box and `fx`, and `bottombar` (sheet tabs
  with Insert Sheet and a status bar with average, count and sum of the
  selection, and zoom). `lang`, `theme`/`color-scheme`, `accent`, `storage-key`,
  `hidden-commands`, plugins with `toolbar` (`use`/`register`), `setNotice`,
  Excel shortcuts, context menu, drag and drop, dialogs (Format
  Cells, Find and Replace, Go To, Insert Function, Column
  Width, Row Height, Properties) and the optional `host` of the host
  app (native dialogs, confirmation on close, frameless window with
  `.uo-wco`). Without the attributes the element stays as it was.
- New editing API (used by the bars and public): range selection
  (`selectRange`, `getSelection`, `selectAll`, Shift/Ctrl+arrows), undo/
  redo, copy/cut/paste with relative references adjusted,
  Paste Values, Fill Down/Right, Clear, `setFormat` (font,
  fill, borders, alignment, wrap, number formats, decimal
  places), merge, freeze panes, gridlines, width/height,
  AutoFit, hide/unhide, find/replace, AutoSum, Insert
  Function, Calculate Now, `newWorkbook`, `addSheet`, `selectionStats`,
  `getProperties`, `isDirty`, events `us-change`, `us-load`,
  `us-theme-change`. Everything goes into the .xlsx: styles.xml gets only the
  new fonts/fills/borders/xfs and the worksheet rewrites
  `<mergeCells>`/`<sheetView>` only when they changed.
- sheet-core: `blankWorkbook`/`blankXlsx` and the `merges`/`sheetView` options
  of `updateWorksheetXml`.
- Desktop app (in Universal Office, see above): the frameless window shows the
  component's bar with the OS controls overlaid; native Open/Save/
  Export, confirmation of unsaved changes on close, and
  spreadsheets opened by the OS come in through the component API (`openWorkbook`)
  instead of a synthetic "drop".

### Slides

- `<universal-slides>` got its own bars, like those of `<universal-write>`
  and on the same framework (`@universal-office/ui`), wired through the same
  attributes: `topbar` (compact bar, the default — logo, editable title above
  the menus "Arquivo · Editar · Exibir · Inserir · Formatar · Slide ·
  Apresentação · Ajuda" (File · Edit · View · Insert · Format · Slide · Slideshow · Help), a tools row that collapses by priority and the
  Present button — or `toolbar="ribbon"`, with the tabs "Página Inicial ·
  Inserir · Design · Transições · Animações · Apresentação de Slides · Exibir" (Home ·
  Insert · Design · Transitions · Animations · Slide Show · View)
  and the contextual Shape Format tab), a THUMBNAILS pane (drag to
  reorder, context menu), a NOTES pane, Slide Sorter
  and Reading modes, Animation Pane (read-only) and `bottombar` (Slide n of N,
  view modes, zoom). `lang`, `theme`/`color-scheme`, `storage-key`,
  `hidden-commands`, plugins with `toolbar` (`use`/`register`), `setNotice`,
  `loadBarKit`, PowerPoint shortcuts (F5, Shift+F5, Ctrl+M, Ctrl+D…), shape
  clipboard, drag and drop (a presentation opens, an image
  goes into the slide), dialogs (Find and Replace, Properties,
  Set Up Slide Show, Size and Position, Link, Shortcuts) and the optional `appHost`
  of the host app (native dialogs, printing, confirmation on
  close, frameless window with `.uo-wco`). Without the attributes the element stays
  as it was.
- Editing goes into the FILE (before, edited text only changed the screen):
  `deck-edit.ts` replaces only the edited span of each part and the model is reread
  from the package. New API (used by the bars and public): `openPresentation`,
  `newPresentation` (Office theme, six layouts, notes master),
  `documentName`, undo/redo, `isDirty`/`markClean`, slides (new from
  a layout, duplicate with the notes, delete, move, hide,
  layout, background, transition — 21 effects —, "Set Up Slide Show"),
  shapes (selection, text box, 28 shapes, image, position/size,
  fill, outline, order, delete with their animations, copy/paste,
  link), shape text formatting, notes, properties,
  find/replace, zoom, `previewSlide`, `getSlideAnimations`. Events
  `us-change`, `us-selection`, `us-dirty`, `us-title-change`, `us-zoom`,
  `us-theme-change`, `us-file-drop`.
- slides-viewer: protected hooks for the editor (`rereadDeck`,
  `rereadSlide`, `slidePartPath`, `deskColor`); the viewer's behavior does not
  change. slides-core: `PptxPackage.fromParts`, and the slide number (field
  `slidenum`) follows slide insertion/deletion/moves.
- Desktop app (in Universal Office, see above): the frameless window shows the
  component's bar with the OS controls overlaid (without the drag
  region injected into the demo);
  native Open/Save (to the same file)/Export, printing through the system
  dialog, confirmation of unsaved changes on close, and presentations
  opened by the OS come in through the component API (`openPresentation`) instead
  of a synthetic "drop". The demo is now just the component.

### Spell checking

- New package `@universal-office/dictionary-es` 0.9.2: the Spanish Hunspell
  dictionary (RLA-ES 2.9, from LibreOffice, `es_ES` variant — LibreOffice's `es/` folder
  has no neutral dictionary), a single one for all variants: `es`, `es-ES`,
  `es-MX`, `es-419`, `es-AR`… resolve to it (id `es`). License
  `(GPL-3.0-or-later OR LGPL-3.0-or-later OR MPL-1.1+)`, with the three texts in
  `licenses/`. Optional like the others: `registerDictionary('es', () =>
  import('@universal-office/dictionary-es'))` or the
  `<base>/dictionary-es/` folder with `spellcheck-url` (`es` is in the default list
  of `PUBLISHED_DICTIONARIES`; with `spellcheck-languages`, list `es`).
- `<universal-write>`: `spellcheck-default-lang` attribute (property
  `spellcheckDefaultLang`), the language of text whose language the document does not state (no
  `w:lang` on the run, in the styles or in the defaults); the default is still `pt-BR`. A
  new document (`newDocument()`) is created in that language: `blankDocxPackage({ lang })`
  writes the `w:lang` of the defaults and `w:themeFontLang`, as Word does with the
  editing language.

### Spanish interface

- Spanish is the third interface language of all web components
  (`<universal-write>`, `<universal-sheet>`, `<universal-slides>`,
  `<universal-document-viewer>`, `<universal-slides-viewer>`,
  `<universal-write-diff>`, the fonts dialog and the mentions plugin):
  `lang="es"` (any region — `es-ES`, `es-419`, `es-MX`…) or a browser
  in Spanish. Menus, ribbon tabs and groups, tooltips, dialogs, panes,
  status bars, notices, error messages, accessibility labels and the
  shortcuts help, in neutral Spanish with the Office terminology in
  Spanish (Archivo, Inicio, Insertar, Diseño, Disposición, Referencias,
  Revisar, Vista; Guardar como, Deshacer, Buscar y reemplazar, Autosuma,
  Presentación con diapositivas…) and the "tú" of Microsoft 365. Measurements in cm and
  decimal comma, as in Portuguese. Keyboard shortcuts do not change.
- `@universal-office/ui`: `UiLang` is now `'en' | 'pt' | 'es'`, with
  `UI_LANGS`, `uiLocale(lang)` and `uiDecimal(lang)`. The internal label tables
  are `Record<UiLang, …>`, so an internal label without Spanish does not
  compile. Nothing breaks for the host: in the `{ en, pt }` label passed to the
  bars (plugins, `Lbl`, `createBarButton`) `es` is optional, and without it the
  Spanish interface shows the English text.
- Write: Spanish names of Word's built-in styles and of the table
  styles (galleries and styles pane; the written `w:name` does not change), texts
  of the watermarks, the header/footer templates, cover page and text
  boxes, the quick tables and the descriptions of formatting revisions
  ("Con formato: …"). The texts that Word writes according to the interface
  language (text box name, "[sin valor]", caption labels,
  "Título N" of the manual table of contents) come out in Spanish with `lang="es"`.
- Sheet: formulas do not change with the interface language — they follow
  `formula-locale`, which has function names in Portuguese (oracle) and in
  English; there is no table of Spanish names (`formula-locale="es-ES"` uses the
  English names with the region's separators: `=SUM(A1;1,5)`).

## 0.9.1 — 2026-10-29

First version published with the final name and license.

### Name and license

- The product is called **UniversalOffice**. npm: scope `@universal-office/*`.
  .NET: projects, assemblies and
  namespaces `UniversalOffice.*` (`OfficeNext.*` is gone; `UniversalOffice.Layout`
  became `UniversalOffice.Fonts`), solution `dotnet/UniversalOffice.sln`.
- **MIT on everything** (`LICENSE` at the root, copyright Eric Kono). Third-party data with their own
  licenses, inside each package: `@universal-office/font-files` =
  `OFL-1.1 AND Apache-2.0` (Caladea is Apache-2.0), Hunspell `MPL-1.1 AND MIT`,
  pt-BR dictionary MPL-1.1, en-US SCOWL license, notices for the hyph-utf8
  hyphenation patterns.

### npm packages

- Published as 0.9.1: `@universal-office/write` (editor, with the
  `./viewer` entry that only registers `<universal-document-viewer>` and shares the
  chunks), `@universal-office/document-viewer` (viewer only),
  `@universal-office/font-files`, `@universal-office/spellcheck`,
  `@universal-office/dictionary-pt-br` and `@universal-office/dictionary-en-us`.
  Published as 0.0.1: `@universal-office/compare` and
  `@universal-office/presentation-viewer`. `@universal-office/sheet` and
  `@universal-office/slides` (0.0.1) are generated, but not published yet.
- The open fonts moved out of each product (they were the same 7 MB in
  each one) into `@universal-office/font-files`, the only dependency of the
  products, at an exact version. The host serves the package folder and points the
  `fonts` attribute to it; without the attribute, the viewer looks for the sibling folder
  `font-files/`.
- Spell checking no longer comes inside write: the engine
  (`@universal-office/spellcheck`) and each dictionary are OPTIONAL packages,
  registered by loader (`registerSpellcheck`, `registerDictionary`, with a
  bundler) or by folder (`spellcheck-url`, `spellcheck-languages`,
  `configureSpellcheck`). Without them there is no spell checking and no error.
- A host that loads by URL: one subdirectory per package,
  `/vendor/universal-office/<package>/` (`write/`, `font-files/`,
  `spellcheck/`, `dictionary-pt-br/`…).
- No vendored tgz: each package comes out of `tools/build-pkg.mjs` in
  `<package>/publish/` and is packed with `npm pack`.

### NuGet packages

- Published as 0.9.1: `UniversalOffice.Core` (Core + Fonts + Pdf +
  DrawingML) and `UniversalOffice.Docx` (Docx + Editing + DocxLayout + DocBinary
  + the docx exporters). `UniversalOffice.Xlsx` and `UniversalOffice.Pptx` are
  packed, but not published yet. Common metadata in
  `dotnet/Directory.Build.props`, an English README inside each package,
  `.snupkg` symbols.
- Server-side .docx editing (`UniversalOffice.Docx.Editing`): sanitizing
  untrusted content with ceilings (`DocxLimits`, `OpcPackageLimits`,
  `OfficeLimitExceededException`), a single rule for relationship targets
  (`OpcPartName.ResolveRelationshipTarget`), templates, `#{...}` parameters,
  plugin inlines (`CustomInlineTag`), plain text, links and bookmarks in the PDF,
  metric substitutes (`SubstituteFontProvider`).
- Nothing consumer-specific in the library: the Quill conversion moved
  to the app that uses it. In its place, the generic API `DocxBodyWriter`,
  `DocxRunFormat`, `DocxParagraphFormat`, `DocxParagraphProperties` and
  `DocxParagraphSamples`.

### Layout (JS and .NET engines, measured in Word)

- `NUMPAGES` excludes the cover when the body has a cover page from Word's
  gallery (sdt "Cover Pages") and the 1st section starts numbering at 0;
  `SECTIONPAGES` never excludes it and is calculated per section.
- Formula fields recalculated like Word (`{ = { NUMPAGES } - 1 }`, the
  total without the cover), with small and safe evaluation.
- A table that continues in another column stays at the new column's x.
- "Inside"/"outside" on an even page: position and inside/outside margins switch
  sides, for text boxes and floating tables.
- Balancing of 3 or more columns at a continuous section break.
- Wrap contour of rotated objects: between 45° and 135° (and 225°–315°) width and
  height are swapped.
- PDF with spaces between words written (search and copy), clickable links and
  bookmarks from the headings.
- Differences that remain: Word prefers the installed (or cloud) font over the
  embedded one; mirrored margins and gutter not applied; rotated box above the
  anchor paragraph; data-bound content controls. Details in the
  `docx-layout` README.

### Editor and viewer for host apps

- Compact bar with the title row above the menus, like Google Docs
  (`compact.titlePlacement: 'above-menus'`, the default when there is a title;
  `'inline'` goes back to a single row); `loadBarKit()` and `createBarRow()` build the same bar
  outside the editor (preview, read-only).
- `side` slot (and `side-start`) in `<universal-write>` and in
  `<universal-document-viewer>`: host side panel below the bars;
  `uw-side-resize` event.
- `setNotice(el, { tone })`: host notice strip between the menus and the
  tools.
- `formatting-policy="template-document"` (and the
  `formattingPolicy` property): the user only formats what the policy allows; paste
  converts the rest.
- `uw-file-drop` (cancelable) and `nofiledrop`: the host decides what to do with
  dropped files.
- Light/dark through the context's CSS `color-scheme` property (`theme` forces it);
  `effectiveTheme` and `uw-theme-change` / `udv-theme-change`.
- Plugins in the viewer too (the same `inlineRenderers`) and a global registry
  (`register`/`unregister`); `isEmpty`, `isDirty`/`setDirty`, `getLinks`,
  `getCustomInlines`; Ctrl+click opens only http/https/mailto, with no opener and no
  Referer.
