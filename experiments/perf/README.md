# Editor performance on slower machines

`harness.mjs` drives Chrome through the DevTools Protocol with the CPU slowed down
(`Emulation.setCPUThrottlingRate`: 4× ≈ mid-range laptop/phone, Lighthouse's default; 6× ≈
a weak machine) and the same emulated network for every run (20 Mbps, 60 ms). It serves the
published site (`dist/site`, Brotli-compressed like a real host) and measures, for any page:

- **load**: bytes transferred (Resource Timing), time until the document is on screen
  (`--ready` expression), time until interactive (end of the last long task before 2 s of
  quiet), total blocking time (TBT), longest task;
- **memory**: JS heap after GC, DOM nodes, canvas pixels;
- **scroll**: frames while scrolling with the wheel;
- **typing** (`--type`): per-key latency from the Event Timing API (input → next frame).

`--profile` / `--profile-typing` save CPU profiles; `out/top.mjs` lists the hot functions.

```shell
node harness.mjs site:/live.html uo --site dist/site --rates 1,4,6 --runs 3 --type --ready "$(cat out/ready-uo.txt)"
```

## What the profiles showed (live editor, `demo.docx`, 8 pages)

1. **15 full layouts while opening the document.** Every font that arrived from the network
   (theme fonts, substitutes, variants the layout asked for, hyphenation patterns) called
   `queueRefresh()`, which only coalesced within one task — so each arrival re-laid out the
   whole document.
2. **The same fonts parsed over and over: 71 GPOS reads for 19 fonts.** The editor builds a
   new `SfntFont` per registration and expanding class kerning into pairs (`parsePairPos`)
   was the top function of the load profile (2.1 s at 4×).
3. **The embedded fonts arrived after the first layout**, forcing another full layout — and,
   in the narrow (phone) view, a title measured with fallback metrics and painted with the
   real font ("Demonstrat ion").
4. Typing: each keystroke re-lays out the whole document, takes an undo snapshot that
   serializes the whole state (`stringifyState`), syncs the toolbar and style lists and reads
   the revisions.

## Changes

- `universal-write.ts`, `queueRefresh()`: background re-layouts wait 80 ms after the last
  request (300 ms at most after the first) and any direct `refresh()` covers a pending one.
- `docx-layout/src/typography/font/sfnt.ts`: GPOS kerning cached per font file
  (`WeakMap` keyed by the bytes; maps are read-only after the constructor, results identical).
- `universal-write.ts`, `loadDocx()`: the embedded fonts' metrics are registered **before** the
  first layout; the browser `FontFace` loads afterwards and only triggers a repaint (a font
  the browser rejects is removed and the layout redone, as before).

Effect while opening: 15 → 5 layouts, 71 → 23 font parses; the editor no longer downloads
substitute fonts the document does not need.

## Results (median of 3 runs)

| | CPU | Before | After |
|---|---|---|---|
| Transferred | — | 5.3 MB | **3.7 MB** |
| JS heap after load | — | 32 MB | **21 MB** |
| Interactive | 1× | 3.8 s | **2.9 s** |
| | 4× | 11.2 s | **5.6 s** |
| | 6× | 16.5 s | 11.7 s |
| Total blocking time | 1× | 0.49 s | **0.21 s** |
| | 4× | 7.7 s | **2.6 s** |
| | 6× | 12.8 s | 8.4 s |
| Longest task | 4× | 1.9 s | **1.2 s** |
| Keys over 50 ms (of ~80 events) | 4× | 18 | **8** |
| Keys over 100 ms | 6× | 24 | 10 |

Scrolling was already smooth (no long tasks at 1×–4×). Canvas memory is 6.6 MB for the
visible pages. 6× varies a lot between runs.

## Next steps (not done)

- **Undo while typing**: each key serializes the whole document state; on long documents
  this grows linearly. Grouping consecutive typing into one undo step (as Word does) or
  storing paragraph-level diffs would remove it — the first changes behavior and needs a
  decision.
- **Incremental layout**: a keystroke re-lays out the whole document (~18 ms at 4× for 8
  pages; linear in document size).
- **Allocation in the TS line breaker**: 182 MB allocated per pass over 13k lines (the C# and
  Rust versions allocate 26–28 MB for the same work, see `experiments/rust-spike`): object
  spreads, filter chains and a new object per placed glyph.
- **Tables**: `columnWidths` and cell layout are ~40% of the remaining load layout at 6×.
- The page starts with a blank document (`newDocument`) and lays it out before `load()`.
- In the narrow (web) view the body of `demo.docx` is drawn in a sans font other than
  Ubuntu — also in the previous build; to look into separately.
