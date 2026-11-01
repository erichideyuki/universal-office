# Spikes: parts of the engine in Rust

Two spikes, each a faithful port of a real part of the TS engine, run on the same inputs
as the existing TS and C# code, with exact parity required. Spike 1: the font core.
Spike 2: line breaking, with CPU **and memory** measured in TS, C# and Rust.

## Spike 1: the font core

Question: with one implementation for every platform, does Rust keep (or improve) the
performance we have today in TS (browser) and C# (server)?

### What was ported

The hottest self-contained part of the layout engine: in a CPU profile of the TS engine on
`demo.docx`, `gpos.js:parsePairPos` was the top function (835 ms over 11 layouts), plus
glyph lookup and advance measurement (`para.js:pushChars`).

- `src/sfnt.rs`, `src/gpos.rs`: a faithful port of `docx-layout/src/typography/font/sfnt.ts`
  and `gpos.ts` (tables, metrics, cmap formats 4 and 12, advances, GPOS pair kerning formats
  1 and 2 with extension lookups, legacy `kern`). Same algorithm (format-2 class kerning
  expanded into pairs, "first pair wins"), same read order, same error handling. VDMX,
  GSUB, COLR and outlines were left out (not used to measure text). ~560 lines of Rust for
  ~700 lines of TS, no dependencies.
- The same workload in all implementations, on the same inputs (`node prep.mjs`: the 20 open
  fonts + the 6 fonts embedded in `demo.docx`, and the document's 8,870 code points of text):
  1. parse the 26 fonts (10×);
  2. measure the text with each font at 11 pt, advance + pair kerning per glyph, the way
     the layout does (230,620 glyphs, 20×).
- Parity: per font, kerning pairs, class-0 entries, `kernInLatin`, `unitsPerEm` and the
  measured width (6 decimals), plus the grand total. **All 7 variants match the TS engine
  exactly.**

### Results (same Windows desktop, Node 24 / V8, .NET 10)

| Implementation | Parse 26 fonts | Measure 230k glyphs |
|---|---|---|
| Rust, native | **33 ms** | **3.3 ms** |
| C#, native (JIT) — `UniversalOffice.Core` | 34 ms | 11.2 ms |
| TS in Node (V8) — `docx-layout` | 164 ms | 11.8 ms |
| **Rust → WebAssembly** (opt-level 3) | **42 ms** | **4.6 ms** |
| Rust → WebAssembly (opt-level s) | 49 ms | 5.0–5.5 ms |
| C# → WebAssembly, AOT (lean) | 131 ms | 15.1 ms |
| C# → WebAssembly, interpreted (lean) | 355 ms | 66–69 ms |

- Rust in WebAssembly vs TS (what the browser runs today): **3.9× faster parsing, 2.6×
  faster measuring**.
- Rust in WebAssembly vs C# in WebAssembly (AOT): 3.1× and 3.3× faster.
- Rust in WebAssembly is only 1.3–1.4× slower than native Rust; native Rust matches C# on
  parsing and is 3.4× faster measuring.
- Start-up: instantiating the Rust module takes ~1 ms (no runtime); the .NET runtime
  starts in 80–300 ms.

Size of the module: Rust 57–59 KB (≈18 KB Brotli) for this slice, against 12.9 KB (4.7 KB
Brotli) of minified TS for `SfntFont` (which also has GSUB, outlines, COLR). Most of the Rust
bytes are a fixed cost (allocator, hash map, parts of std), so the slice says little about
the size of a whole engine; the C# runtime alone is ~1.5 MB Brotli.

### Lessons

1. **Performance in Rust is not automatic.** The first Rust build parsed at 61 ms, slower
   than C#. The cause was the integer hash I wrote (classic FxHash): for keys
   `g1·65536 + g2` its low bits depend only on `g2`, piling pairs into the same buckets. The
   final rotation that `rustc-hash` 2 adds fixed it (61 → 33 ms). In a real port, use the
   standard crates (`rustc-hash`, `hashbrown`) and profile.
2. **The parity infrastructure is the asset that makes a port safe.** Exact parity on the
   first run, checked by numbers, not by eye. The engine's parity tests and Word oracle
   would play the same role for the full port.
3. **Part of the cost is algorithmic, in every language.** Expanding format-2 class kerning
   into ~170k pairs per font is what makes parsing expensive; keeping the class tables
   would cut it in TS, C# and Rust alike.
4. **What this slice does not cover:** layout builds and mutates trees of objects (paragraphs,
   lines, runs, table cells). GC languages allocate those cheaply; in Rust the idiomatic
   shape is arenas with indices, a redesign rather than a translation. That is where the
   remaining risk (and most of the porting cost) is.

### Run it

```shell
node prep.mjs
cargo build --release && ./target/release/bench
cargo build --target wasm32-unknown-unknown --profile wasm-fast && node bench-wasm.mjs wasm-fast
node bench-ts.mjs
dotnet run -c Release --project csharp/FontBench -- <this folder>
node bench-cs-wasm.mjs ../wasm-poc/out/lean-aot
```

`bench-ts.mjs` uses the built `dist/` of `write/packages/docx-layout`; `bench-cs-wasm.mjs`
uses the `wasm-poc` builds (`FontBenchExports.cs` there).

## Spike 2: line breaking (CPU and memory)

### What was ported

`docx-layout/src/lines.ts` (the `LineBreaker`: greedy breaking with Word's rules, tab stops
— custom, default grid, right/center/decimal pending tabs, absolute tabs, tabs past the
margin and beside floats —, justification with space compression, hanging punctuation,
Asian-layout compression, numbering markers, line height and baseline by line rule and
document grid, alignment) and the hyphenation choice of `hyphenate.ts`
(`hyphenCandidates`, `chooseHyphenation`). ~620 lines of Rust (`src/linebreak.rs`,
`src/linedata.rs`) for ~760 lines of TS.

### How it is compared

- `lines/capture.mjs` runs the TS engine on the Word oracle cases (369 documents from 244 of
  the 259 oracle groups; documents with embedded fonts or over 6,000 glyphs left out) and
  records every input of the line breaker — the prepared paragraphs, the sequence of
  `new` / `next(frame)` / `reset(mark)` calls — and every line it returned: 9,556
  paragraphs, 300,247 glyphs, 13,353 lines. The corpus covers automatic hyphenation (219
  lines), numbering markers, tabs, justification, page/column breaks and every glyph kind.
- Each implementation replays exactly those calls with **its own real code**: TS runs the
  engine's `LineBreaker` (dist) and C# the `LineBreaker` of `UniversalOffice.Docx`, both with
  real font faces from the system provider; Rust runs the port (the two font values it needs —
  hyphen width and face metrics — are precomputed in the capture; the font core itself was
  spike 1). Rust reads a binary copy of the capture (`lines/pack.mjs`), TS and C# the JSON.
- Parity: every line (start, end, width, x, ascent, descent, height, baseline, flags, and
  every placed glyph with x, width and leader) must equal the capture, exact floats.
  **All implementations: 0 mismatches in 13,353 lines.**
- Per replay pass (all 13,353 lines): wall time (best and median of 30), process CPU time,
  memory allocated during the pass (TS: V8 GC profiler; C#: `GC.GetAllocatedBytesForCurrentThread`;
  Rust: a counting allocator) and memory retained by the produced lines (heap after GC
  with and without them; Rust: live bytes).

### Results (same Windows desktop, Node 24 / V8, .NET 10)

| Implementation | Best | Median | CPU / pass | Allocated / pass | Retained (lines) |
|---|---|---|---|---|---|
| TS in Node (V8) — `docx-layout` | 83 ms | 90 ms | 100–105 ms | **182 MB** | 30.6 MB |
| C#, native (JIT) — `UniversalOffice.Docx` | 29–30 ms | 45 ms | 56–62 ms | 26.4 MB | 19.8 MB |
| Rust, native | 10.2 ms | 10.8 ms | 10.4–10.9 ms | 27.9 MB | 15.2 MB |
| **Rust → WebAssembly** (opt-level 3) | **7.9 ms** | **8.2 ms** | — | 27.4 MB | 14.8 MB |
| Rust → WebAssembly (opt-level s) | 17.3 ms | 18.0 ms | — | 27.4 MB | 14.8 MB |

- In the browser's engine (V8), Rust in WebAssembly breaks the lines **~11× faster** than the
  TS engine and allocates **6.6× less** memory per pass; the lines it keeps take half the
  memory (14.8 vs 30.6 MB).
- On the server, native Rust is ~3× faster than C# (best) and ~4× (median), with less CPU
  (the .NET and V8 GC threads add CPU beyond the wall time) and ~25% less retained memory.
- C# allocates about as little as Rust here: the TS port's 182 MB per pass come from its
  style (object spreads, `filter`/`some` chains, closures, a new object per placed glyph and
  per `nextTab`), not from the language — an optimization opportunity for the current TS
  engine regardless of the Rust decision, since every allocation is GC pressure in the
  editor.
- Native Rust slower than Rust in WebAssembly is an artifact of the Windows system
  allocator (the pass makes many small allocations; in WebAssembly Rust uses its own
  allocator). The allocation counter costs ~0.4 ms (measured with it off). A faster native
  allocator (`mimalloc`) was not tried here.
- `opt-level = "s"` costs 2.2× on this code; the speed-optimized module is 100 KB
  (both spikes together), the size-optimized one 86 KB.
- Process peak memory (RSS) is not comparable across the three: it is dominated by loading
  the inputs (TS and C# parse a 56 MB JSON into object graphs; Rust reads a 39 MB binary).
- **Not measured:** C# compiled to WebAssembly for this spike (it would need the 33 system
  font faces passed in as bytes, including `.ttc` collections); spike 1 put C# AOT in
  WebAssembly at ~3× slower than Rust in WebAssembly.

### Lessons

1. The port of the most rule-dense part of the engine came out with exact parity on the
   first run, thanks to the capture/replay with exact comparison. That is the method for a
   full migration: capture each stage of the TS engine, port it, replay until identical.
2. The arena/tree concern did not bite here: the breaker works on flat arrays (glyphs,
   placed glyphs by index), which suits Rust well. The remaining risk is the engine's
   object graph above this level (paragraph → line → page placement, tables, floats).
3. For the current TS engine, the allocation volume of the breaker is worth attacking now.

### Run it

```shell
node lines/capture.mjs && node lines/pack.mjs
node --expose-gc lines/replay-ts.mjs 30
dotnet run -c Release --project csharp/LinesBench -- <this folder> 30
cargo build --release && ./target/release/lines 30
cargo build --release --no-default-features --bin lines && ./target/release/lines 30   # time without the counter
cargo build --lib --target wasm32-unknown-unknown --profile wasm-fast && node lines/replay-wasm.mjs wasm-fast 30
```
