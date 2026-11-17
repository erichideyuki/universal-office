# uo-pptx — parity captures (B1 read path, B2 `.ppt`)

Record of the capture harness for the Slides port (plan `core/docs/slides-sheet-port-plan.md` §4.3, §6 B1–B2). First
part only: deck generator, model capture and `.ppt` conversion capture. The advance tables and the draw-op stream
recorder wait for the display-list design (see "Next wave").

Everything is Node under `core/crates/uo-pptx/js/`, fixtures under `core/crates/uo-pptx/tests/captures/`.
The TS reference is the frozen 0.9.2 checkout (`UO_TS_ROOT` or `--ts-root`, default
`D:/Projects/OfficeNext/.claude/worktrees/ts-ref-092`, commit `b92b60fae4c1c612de1de3174c199052d43b9230`, `dist` built for
slides-core). The scripts import `slides/packages/slides-core/dist/index.js` and need no DOM, canvas or npm install:
the readers run in plain Node (a slide reads in 3–40 ms).

## Files

| File | Role |
|---|---|
| `js/gen-decks.mjs` | seeded deck generator CLI (`--list`, `--gen <group>`, `--out`, `--check`, `--only`, `--stats`) |
| `js/lib/gen/groups.mjs` | registry of groups: seed range, profile, size, CI or local |
| `js/lib/gen/{b1,parts,shapes,dml,text,anim,rand}.mjs` | the B1 builder: package skeleton, shapes, DrawingML fragments, text, animations/transitions |
| `js/lib/{zip,png,canon-json,common}.mjs` | deterministic zip writer, tiny PNGs, canonical JSON, plumbing |
| `js/capture-model.mjs` | TS `Presentation` / `Slide` models → capture (+ optional detail sidecar) |
| `js/capture-ppt.mjs` | TS `.ppt` → `.pptx` parts → capture |
| `js/capture-timing.mjs`, `js/lib/gen/b6.mjs` | B6: `parseSlideTiming` per slide, four playback timelines, states, hook sweeps, four `SlideShow` scripts and the slot recording (every anim hook call of `paintSlideTo`: base in, `PartState` out); formula corpus and numeric helpers (`--formulas`). Replayed by `tests/timing_replay.rs`; record `uo-pptx.md` "B6 — timing and playback" |
| `tests/captures/timing-{samples,b1a,b6a}.{capture,frames.gz}`, `timing-formula.{capture,formulas.gz}`, `model-b6a.capture`, `decks/b6a/` | the B6 captures: digests per deck (`t<N> tl.<v> st.<v> hk sh.<k> fr`), the hook-call sidecar (tab-separated lines, numbers as the 16 hex digits of the f64 bits, 1.4 MB) and the formula results (31 KB); 3.7 MB with the decks |
| `tests/captures/decks/b1a/NNNN.pptx` | the 200 committed generated decks (CI) |
| `tests/captures/model-samples.capture`, `model-samples.json.gz` | 53 samples: digests, and the canonical JSON text itself |
| `tests/captures/model-b1a.capture` | the 200 generated decks: digests |
| `tests/captures/ppt-samples.capture` | the 25 tracked `.ppt` samples: part names, hashes, small-part bytes |
| `js/capture-drawops.mjs`, `js/lib/{record-ctx,ts-paint,rust-paint,opstream,views}.mjs` | R-S4: draw-op streams of the frozen TS painter over a recording canvas (`uo-render.md` "R-S3/R-S4") |
| `js/capture-textlayout.mjs` | B3: `layoutTextBody` of every text body with the frozen measurers over the real advances of the open fonts, the measureText log and result digests (`uo-dml-render.md` "B3 — text layout"); replayed by `tests/text_layout.rs` |
| `js/lib/gen/b3.mjs` | the B3 slide builder (`profile.slide` hook of `b1.mjs`): text-heavy slides for group `b3a` |
| `tests/captures/decks/b3a/NNNN.pptx`, `tests/captures/textlayout-{samples,b1a,b3a,b3a-nokerning-noseg}.{capture,measures.tsv.gz}` | the 120 text-heavy decks and the text-layout captures (5.1 MB with the logs) |
| `js/compare-drawops.mjs`, `js/selftest-drawops.mjs` | the Rust-list-vs-TS comparison (B4) and the self-test (`node --test`) |
| `tests/captures/drawops-samples.{capture,ops.gz,measures.tsv.gz}`, `drawops-b1a.*` | the draw-op streams (draw form) per slide and view, digests, the measureText log |
| `core/crates/uo-charts/js/capture-charts.mjs`, `tests/captures/charts-{c1a,samples,diff}.*` | uo-charts C-0 (group `c1a`: 600 generated charts, seed base 700000; 26 sample charts; 2,760 differential cases): chart model, host log, ops, measure asks, bounds, layout facets; it reuses `lib/{record-ctx,opstream,canon-json,common}.mjs` and `gen/rand.mjs` of this folder; the recorder gained the opt-in capabilities `roundRect`, `strictFont` and the measurer's font box (record `uo-charts.md` "C-0 — harness") |

## Commands

```text
# regenerate everything committed (about 25 s; seconds for each model capture, 14 s for the .ppt one)
node core/crates/uo-pptx/js/gen-decks.mjs --gen b1a                       # decks/b1a/*.pptx (same seed → same bytes)
node core/crates/uo-pptx/js/capture-model.mjs --samples --detail core/crates/uo-pptx/tests/captures/model-samples.json.gz
node core/crates/uo-pptx/js/capture-model.mjs --gen b1a --decks core/crates/uo-pptx/tests/captures/decks
node core/crates/uo-pptx/js/capture-ppt.mjs
# the committed decks still match the generator (exit 1 and `DIFFERS:` lines otherwise)
node core/crates/uo-pptx/js/gen-decks.mjs --gen b1a --check
# B6 timing and playback (about 40 s for everything)
node core/crates/uo-pptx/js/gen-decks.mjs --gen b6a
node core/crates/uo-pptx/js/capture-model.mjs --gen b6a --decks core/crates/uo-pptx/tests/captures/decks
node core/crates/uo-pptx/js/capture-timing.mjs --samples
node core/crates/uo-pptx/js/capture-timing.mjs --gen b6a --decks core/crates/uo-pptx/tests/captures/decks
node core/crates/uo-pptx/js/capture-timing.mjs --gen b1a --decks core/crates/uo-pptx/tests/captures/decks
node core/crates/uo-pptx/js/capture-timing.mjs --formulas --decks core/crates/uo-pptx/tests/captures/decks
# B3 text layout (about 25 s for all four captures)
node core/crates/uo-pptx/js/gen-decks.mjs --gen b3a
node core/crates/uo-pptx/js/capture-textlayout.mjs --samples
node core/crates/uo-pptx/js/capture-textlayout.mjs --gen b1a --decks core/crates/uo-pptx/tests/captures/decks
node core/crates/uo-pptx/js/capture-textlayout.mjs --gen b3a --decks core/crates/uo-pptx/tests/captures/decks
node core/crates/uo-pptx/js/capture-textlayout.mjs --gen b3a --decks core/crates/uo-pptx/tests/captures/decks --caps nokerning,noseg
# local only: 2,000 decks, about 1 minute, nothing committed
node --max-old-space-size=8192 core/crates/uo-pptx/js/capture-model.mjs --gen b1-big --out D:/tmp/model-b1-big.capture
node core/crates/uo-pptx/js/gen-decks.mjs --gen b1-big --out D:/tmp/decks   # the decks themselves, for the Rust side to read
```

Captures are run twice byte-identical apart from the `@platform` line (checked for all four).

## The generator (`gen-decks.mjs`)

Pure Node, no dependencies. The PRNG is the xorshift32 of `uo-testkit` (`Rng`, seed mixed and warmed up so consecutive
seeds are unrelated); no `Date`, no `Math.random`; the zip writer stamps 1980-01-01 and uses `zlib` deflate level 6;
the pictures are 4–24 px PNGs written in code (`sqrt`, never `hypot`, so no platform-dependent math). Each deck is a
valid package: content types, rels, docProps, presentation, presProps, tableStyles, theme(s), a master with 5
placeholders (optional decoration, dark or light `clrMap`), 3–5 layouts (Title Slide, Title and Content, Two Content,
Title Only, Blank; some with `showMasterSp="0"`, decoration or a `clrMapOvr`), 1–4 slides, optional notes master and
notes slides. Fonts appear by name only: Liberation Sans/Serif/Mono, Carlito, Caladea and the theme refs
(`+mj-lt`/`+mn-lt`); no font file, no embedded font part.

Covered per slide (all measured in the TS models, see "Coverage"): 140 preset geometries with adjust values plus
`custGeom` (line, quadratic and cubic Bézier, arc, per-path fill/stroke modes); solid, linear and path gradients
(rotWithShape, flip, `tileRect`), 54 patterns, picture fills (stretch, `srcRect`, tile, blip effects: alphaModFix,
grayscl, biLevel, lum, duotone, clrChange), `grpFill`; lines (width, cap, compound, dash presets and custom dash, join,
head/tail arrows of every type and size); shadows (outer, inner, preset), glow, soft edge, reflection, blur; bevel,
extrusion and camera (`scene3d`/`sp3d`); `p:style` refs; groups (nested, scaled child space, rotated, flipped, `grpFill`),
connectors (all `bent`/`curved` presets, arrows), pictures (crop, geometry, outline, effects), rotations and flips on
shapes; text bodies (insets, anchors, `vert` modes, columns, `rot`, all autofit forms including `fontScale`/`lnSpcReduction`,
`prstTxWarp`, `lstStyle`), paragraphs (levels, alignment, margins/indent, line spacing and spacing before/after in
percent and points, tabs, `defRPr`, bullets: none, char, auto-number with `startAt`, picture bullet, colour/size/font),
runs (size, bold, italic, underline variants, strike, spacing, baseline, caps, kerning, colour with every modifier kind,
gradient text fill, outline, highlight, shadow, latin/ea/cs/sym fonts, hyperlinks through external rels, fields
`slidenum`/`datetime*`, line breaks, Greek/Cyrillic/Arabic/Hebrew/CJK/Indic/Thai/emoji/combining text); placeholders
inheriting geometry and properties from layouts and masters (and overriding them); colours as sRGB, scheme, preset,
system, scRGB and HSL with modifiers (lumMod, lumOff, tint, shade, alpha, satMod, hueOff, comp, inv, gray, red/green/blue
families…); tables (11 style GUIDs, `firstRow`…`bandCol` flags, `gridSpan`/`rowSpan`/`hMerge`/`vMerge`, per-cell borders,
fills, margins, anchors, vertical text); animations (entrance appear/fade/wipe/split/fly/zoom/wheel, emphasis
spin/grow/pulse/colour, exit disappear/fade/wipe, motion path, click/with/after triggers, interactive sequences,
`bldLst` with `animBg`); transitions (22 basic kinds plus p14 ones in `mc:AlternateContent`, `spd`, `advClick`, `advTm`);
slide backgrounds (`bgPr`, `bgRef`), hidden slides, presentation `showPr` (loop, kiosk, range, no timings), notes.

Groups (disjoint seed ranges, so adding a group never changes another; each deck `i` uses seed `seedBase + i`):

| Group | Seeds | Decks | Use |
|---|---|---|---|
| `b1a` | 100000–100199 | 200 (4.6 MB) | CI: decks committed |
| `b3a` | 300000–300119 | 120 (2.8 MB) | CI: decks committed; text-heavy slides (B3) |
| `b5a` | 500000–500149 | 150 (3.7 MB) | CI: SmartArt decks (B5a), `js/lib/gen/b5a.mjs`; capture `model-b5a.capture` |
| `b6a` | 600000–600099 | 100 (1.8 MB) | CI: timing and playback decks (B6), `js/lib/gen/b6.mjs`: every effect class, iterate, triggers, media, every transition kind; captures `model-b6a.capture`, `timing-b6a.*` |
| `b1-big` | 200000–201999 | 2,000, larger decks | local only |

B3–B6 add groups in `groups.mjs` (own seed range, own builder or profile in `lib/gen/`): text layout stress, render,
SmartArt/metafiles/charts, timing. Not covered by B1 and left to those waves: charts, SmartArt, media, EMF/WMF/PICT,
embedded fonts, OLE, `.ppt` decks (the PowerPoint COM `SaveAs` step, Windows-only; `gen-stress*.ps1` of
`slides/tools/ppt-lab`).

Coverage (from the TS models of `b1a`): 500 slides, 5,360 shapes; 274 distinct generator feature tags (recorded in the
`@coverage` line of `model-b1a.capture`); every group above shows up in the model (fills of all types incl. pattern
and path gradient, `shadow`/`glow`/`softEdge`/`reflection`/`innerShadow`, `bevel`/`extrusion`/`camRot`, `custGeom`,
`table` with `hLines`, `txBody.warp`/`numCol`/`vert`/`fontScale`, bullets of all kinds, `timing.steps`/`interactive`/
`transition`, `bgImage`, inherited decoration, 4,713 bulleted paragraphs, 11,601 runs). Every part of every deck parses
with the TS `parseXml`.

## `capture-model.mjs`

Reads each deck with `PptxPackage.load` (which converts a `.ppt` first), `readPresentation`, `readSlide(pkg, ref.target)`
and `readSlideNotes`, and writes the canonical JSON of `Presentation` and of `{ notes, slide }` per slide
(`lib/canon-json.mjs`). The capture stores the SHA-256 of those texts; the detail sidecar (samples only) stores the texts.

Canonical JSON rules (the Rust side must print the same text from its model):

- object keys sorted by UTF-16 code unit, `{"k":v,…}`, no whitespace; a key whose value is `undefined` is omitted;
- arrays `[a,b]`; numbers = JavaScript's shortest round-trip decimal (`String(x)`: `1e+21`, `1.5e-7`, `5`), `-0` kept;
  NaN/±Infinity as `{"$num":"NaN"|"Infinity"|"-Infinity"}` (none occurs in the committed decks; `repeat === Infinity`
  can occur in timing nodes);
- strings through `JSON.stringify` (lone surrogates as `\udXXX`); a string longer than 4096 UTF-16 units is replaced by
  `{"$text":<UTF-16 length>,"sha256":<hex of its UTF-8>}` (the chart XML, `chartXml`);
- `Uint8Array` (image bytes) as `{"$bytes":<len>,"sha256":<hex>}` — the bytes are not stored;
- `Map`/`Set` as `{"$map":[[k,v]…]}` / `{"$set":[…]}` (none occur today); shared references are written in full at every
  occurrence (the theme object shared by shapes, an `ImageRef` shared by shapes: identity is not recorded); a cycle throws.

Records (`samples/<path>` and `gen/<group>/<NNNN>`), facets: `input` (`len:sha256[0..16]` of the deck), `seed` (generated),
`read` (`ok` or `err:<message>`), `pres`, `slides` (count), `s<N>` (slide N), `model` (one hash over everything),
`dropped`. Digest = `len:sha256[0..32]` of the UTF-8 text.

Added for the Rust read path (B1b), existing facets unchanged: `n<N>` = the slide record without `timing` (`parseSlideTiming` is B6), `nmodel` = `model` over `pres` + the `n<N>` texts. Rust compares `input read pres slides n<N> nmodel`, and since B6 (`parseSlideTiming` ported) also `s<N>` and `model`; `dropped` (what the TS canon dropped) is not replayed; the 25 `.ppt` records wait for B2's replay.

A slide that throws is `s<N>=err:<message>` and the deck still
records the other slides (no sample or generated deck throws today: 0 errors in 253 decks).

### What the canonical JSON drops or digests (listed in each record's `dropped`)

Nothing in the TS `Slide`/`Presentation` model is DOM-only or non-deterministic: the readers return plain data. What the
canon does not keep verbatim:

1. `undefined`-valued keys (TS optional fields set to `undefined`; they appear in 7 of the 53 samples and 14 of the 200 generated decks)
   — omitted, i.e. JSON semantics: absent and `undefined` are the same;
2. image/media bytes (`ImageRef.bytes`, 203 decks) — digest only (`bytes->digest`); Rust compares the same digest;
3. strings over 4096 UTF-16 units (`long-text->digest`, 5 records) — the chart XML in `Shape.chartXml`;
4. object identity (shared theme / image objects) — expanded;
5. the `@platform` line of the capture (Node version, CPU) is metadata, not part of any digest.

Not dropped but worth knowing: `Theme.fillStyles`/`bgFillStyles`/`effectStyles` and `Shape.chartTheme` are raw `XmlElement`
trees (`{name, attrs, children}`) and are part of the digest, so the Rust reader must keep its XML parser's tree the same
(prefixes literal, attribute map, text children as strings, entity decoding of the TS `parseXml`).

### Known non-determinism

- The `.ppt` samples go through the TS converter inside `capture-model.mjs`, which numbers `a:fld` GUIDs from a
  process-global counter (below); the model does not contain those GUIDs, so the model digests are order-independent
  (checked: the detail text has no `B6F15528`).
- Nothing else: both model captures were repeated and are identical apart from `@platform`.

## `capture-ppt.mjs`

For every tracked `.ppt` (25: 4 in `legacy/`, 21 in `playback/`): part names in archive order, SHA-256 and
length of each of the 1,082 parts, and for the 901 parts of at most 8 KiB their bytes (`b<N>=z:<base64 raw-deflate>`).
Same idea as `slides/tools/ppt-lab/model-regress.mjs`, with the bytes kept for diffing.

**The capture is date-bound** (found in B2): 30 parts carry the automatic date field, which the TS fills from
`new Date()`. Since 2026-11-17 the worker freezes `Date` at local noon of `--today` (default `2026-11-17`, the day of the
committed capture, recorded as `@today`), and the Rust replay pins the same day through `ConvertOptions.today`; a
re-run gave the same lines plus the `@today` meta line.

**Problem found: the TS converter is not deterministic across conversions in one process.** `ppt/textconv.ts` writes the
GUID of every `a:fld` from a module-global counter (`fieldSeq`, `{B6F15528-21DE-4FAA-801E-634DDDAF<seq:04X>}`, wraps at
0xFFFF, never reset), so converting the same `.ppt` twice gives different bytes in the masters/slides with fields
(`slideMaster1.xml`, `slides/slide2.xml` of `lo-footer-test.ppt` differ on the second run). `model-regress.mjs` hides this
because it converts a fixed list in one process. This capture converts every deck in a fresh worker thread (fresh
module: counter at 0) and twice, and fails if the parts differ. Rust contract: the converter keeps the counter per
conversion and starts at 1 (the first field gets `…DAAF0001`). The product behaviour (the second deck opened in a
session gets other GUIDs) is harmless and not worth keeping. The whole zip is not stable either (fflate stamps the
current time in the entry headers), so the container has no hash: the parts are the contract (the Rust side may
zip with any writer; it compares the parts).

Errors: none of the 25 decks fails to convert. The converter's error paths (`PptFormatError`) are not exercised by the
tracked samples; mutations of the `.ppt` samples are a candidate for B2 (Write's `capture-doc.mjs` does this for `.doc`).

## Replay in the Rust crate (written 2026-11-17: `core/crates/uo-pptx/tests/replay.rs`; the sketch below is the original plan)

```text
let cap = uo_testkit::Capture::read(captures_dir.join("model-b1a.capture"))?;
cap.require_platform(..)?;   // optional: nothing here depends on platform math
let mut replay = uo_testkit::Replay::new(&cap);
for rec in cap.records() {
    let deck = std::fs::read(decks_dir.join(deck_file(&rec.key)))?;          // gen/b1a/0042 → decks/b1a/0042.pptx
    replay.check(&rec.key, "input", &bytes_digest(&deck));                    // len:sha256[0..16]; proves the input
    match uo_pptx::Package::load(&deck).and_then(|p| model_texts(&p)) {      // read path, then canonical JSON
        Ok(m) => { replay.check(key, "read", "ok"); replay.check(key, "pres", &text_digest(&m.pres));
                   for (i, s) in m.slides.iter().enumerate() { replay.check(key, &format!("s{i}"), &text_digest(s)); }
                   replay.check(key, "slides", &m.slides.len().to_string()); replay.check(key, "model", &m.model_hash); }
        Err(e) => replay.check(key, "read", &format!("err:{e}")),
    }
}
replay.finish().assert_clean();
```

- `canon_json(&Value)` in the Rust crate prints the rules above (sorted UTF-16 keys, ECMAScript number formatting through
  `uo-jsnum`, `-0`, the `$bytes`/`$text`/`$num` forms). On a mismatch, `model-samples.json.gz` gives the TS text of any
  sample (`{ "<key>": { "pres": …, "slides": [ … ] } }`): diff the first differing path. For a generated deck, run the
  capture with `--only <key> --detail <file>` locally to get the text.
- Error facets compare the TS message text exactly (they are Portuguese strings from the reader); a Rust error must carry
  the same message or the replay lists the difference as an accepted, named one (never a tolerance).
- `.ppt` samples in `model-samples.capture` can only replay after B2 (their model is read from the converted `.pptx`);
  until then `replay.skip(key, facet)` for keys ending in `.ppt`. `ppt-samples.capture` compares per part: `parts`
  (count and names in order), then `p<N>` hashes, with `b<N>` inflated (raw deflate) to print the first differing byte.
- Large local runs: `UO_PPTX_DECKS=<dir>` + the capture from `--gen b1-big`, behind `opt_in` like Write's `UO_WC_*` flags.

## Next wave (waits for the display-list design)

- **Advance tables** — DONE in B3 (2026-11-17): `js/capture-textlayout.mjs` measures with the real advances of the open fonts (the
  layout's questions and answers in `*.measures.tsv.gz`), group `b3a` adds the text-heavy decks. Original note: the recorder of every measurement the TS layout asks for (`layoutTextBody` measures per
  font and character at 1000 px rounded to 1/8 pt, kerning deltas, whole-string widths, `actualBoundingBox` for warp), taken
  from the real TS run over the generated decks. The generator already produces the text stress (scripts, sizes, spacing,
  vertical text, columns, autofit, warp, bullets, tabs); B3 adds a group of text-only decks with a wider font/size matrix.
- **Draw-op stream** over `paintSlideTo` (primary B4 gate): **DONE in R-S4** (2026-11-17): `js/lib/record-ctx.mjs` (the recording
  context, capability set pinned), `js/capture-drawops.mjs`, `tests/captures/drawops-{samples,b1a}.*` (53 samples at two views and
  the first 100 generated decks, 6.5 MB), `js/compare-drawops.mjs` (the Rust-list-vs-TS comparison, waits for the B4 producer) and
  `js/selftest-drawops.mjs`; B4 (2026-11-17) regenerated the two drawops captures after one recorder change (the recipe hash of an offscreen canvas skips `measure` and `set` lines, `uo-dml-render.md` "B4 — render") and added the Rust side `js/emit-uolf.mjs` + `uo-pptx/examples/emit_uolf.rs` (1,166/1,202 and 246/246 equal; the 36 differing views are chart frames); the format, forms, the measureText log and what B4 needs are in `uo-render.md` "R-S3/R-S4". Still
  open: Chromium pixels as the second gate (opt-in, like Write's); the 100-deck cut of b1a (all 200 decks and both views regenerate
  with `--max`/`--views`, 27.5 MB, local only); the advance tables below need a REAL canvas or `uo-fonts` (the committed measureText
  log records what the TS asks for, answered by a deterministic stub).
- Timing/playback model and timeline (`buildPlaybackTimeline`, `SlideShow` frames) for B6: `slide.timing` is already in the
  model digests; the timeline needs its own capture next to the CreateVideo oracle.
- Generator groups for B4–B6: charts (the lost `gen-charts.cjs` redone), SmartArt `dsp:spTree`, media, metafiles, embedded
  fonts (never font files in the repo; names only), `.ppt` through PowerPoint COM on this machine (Windows-only, outside CI).
- Mutated `.pptx`/`.ppt` inputs (byte-level, like `capture-doc.mjs`) for the reader's error paths and no-panic tests.

## B5b captures (2026-11-17)

- `tests/captures/meta-b5b.{capture,ops.gz,inputs}` (3.9 MB): the frozen TS rasterizers (`raster.ts` `rasterizeSpecialImage`: metafiles, PICT, TIFF) over the recording context for 873 synthetic files (`js/lib/gen/meta-*.mjs`, seeds
  700000.., EMF/EMF+/WMF/PICT/TIFF/EOT and damaged copies), 1,267 rasterizations; `capture-meta.mjs` writes it (the metafile module is imported fresh per case: its font memo is module state), `compare-meta.mjs` compares the Rust
  recipes replayed by `uo-wasm/js/meta-player.ts` line by line, `diff-meta.mjs` shows the first difference of one file, `coverage-meta.mjs` lists the TS lines the set misses. EOT cases store the sfnt digest, TIFF the RGBA digest.
- `tests/captures/drawops-b5b.*` and `decks/b5b/` (40 decks, 0.9 MB): the draw-op stream of slides whose pictures are those files (`js/lib/gen/b5b.mjs`, group `b5b`), recorded with `capture-drawops.mjs --gen b5b --special`
  (the special pictures are rasterized as the viewer does, at the display size of their first use: `lib/ts-special.mjs`), compared with `emit-uolf.mjs` + `compare-drawops.mjs` (the frame files flag recipe bitmaps in bit 31 of the length).
  The two older drawops captures are unchanged (their decks have no such pictures).

## C-6 additions (charts in slides, 2026-11-17)

- Group `c1d` (`js/lib/gen/c1d.mjs`, seeds 800000.., 60 decks, 3.3 MB under `tests/captures/decks/c1d/`): slides with 1-2 charts each from the `uo-charts` generator (`crates/uo-charts/js/gen/charts.mjs`, the `c:chartSpace` families; the chartex ones are skipped, the TS reader does not read them), their pictures as chart-local relationships, `themeOverride` parts for the `alt` theme, a random shape now and then. Captured with `capture-drawops.mjs --gen c1d --decks core/crates/uo-pptx/tests/captures/decks` into `drawops-c1d.*` (182 slide views, 244,618 draw lines, 1.8 MB `.ops.gz`); compared with `emit-uolf.mjs` + `compare-drawops.mjs` (182/182) and by `slides_drawops.test.mjs` through the wasm module.
- `record-ctx.mjs` `DEFAULT_CAPS` gained `roundRect: true` and `strictFont: true` (they were opt-in for the chart captures); `drawops-samples.*` was re-recorded (only its 36 chart views change; `drawops-b1a` is unchanged). The parity stub answers `Metrics` (`tests/support/stub_measure.rs`).
