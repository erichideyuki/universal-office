# uo-image — raster image codecs

Record of the rust-port workflow (hybrid mode, milestone 1, frontier I of uo-pdf §12). Status: **port, parity, memory and
no-panic tests done (2026-11-03); fuzz run 19 M executions without a crash; waiting for the milestone-1 gate.** The plan is the
brief of uo-pdf §10 (approved as PQ3 at the milestone-1 gate); this record lists everything decided outside it.

## Scope

- **TS covered**: `write/packages/docx-layout/src/images.ts` (frozen 0.9.x): `jpegInfo`, `imagePixelSize`, `decodePng`,
  `decodeGif` + `lzw`, `decodeBmp`, `decodeTiff` + `tiffLzw` + `packBits`, `decodeRaster`. The readers of these in the
  engine are `pdf.ts` (`imageFor`, `rgbaOf`: JPEG embedded as it is, everything else decoded to RGBA), `page-fill.ts`
  (`decodePng`/`decodeGif` for the page-fill patterns), `canvas.ts` and `pdf.ts:424` (`imagePixelSize` for blip tiles) and the
  editor's `decodeImage` fallback (TIFF, which the browser does not decode).
- **Encoders**: there are none to port. The TS never encodes an image (`embedRgba` hands raw RGBA to the PDF writer, which
  deflates it with `uo_opc::flate`; the editor's `encodePng` is the browser canvas). The "decoders/encoders" of the brief are
  therefore decoders only.
- **C# (type guide)**: nothing beyond the TS. `Raster.cs` (`DecodePng` without Adam7, `JpegSize`; the converter's PDFs) and the
  Slides `export/raster.ts` it ports are a **separate, older** variant (8-bit only, Adam7 → `null`); per PQ3 they wait for the
  converter port and are not here.
- **Left for other modules**: EMF/WMF (`uo-docx-layout::metafile`, LQ7), the picture adjustments (`applyImageEffects`,
  `applyPictureAdjust`, `effectDecode`: `uo-drawingml::blip`, done), the DPI read of `page-fill.ts` `imageInfo` (it reads the
  `pHYs` and JFIF chunks itself: layout), the BLIP inflate of the OfficeArt readers (`uo_opc::flate::inflate_capped`), the PDF
  embedding (`uo-pdf`).
- **Reuse (Sheet, Slides, converter)**: nothing in the crate assumes Word. Sheet and Slides pictures, the converter's PDFs and
  the editor's TIFF fallback use the same decoders; `Rgba` is the contract.

## Public API

```rust
pub struct Rgba { pub width: u32, pub height: u32, pub data: Vec<u8> }            // RGBA8, row by row
pub struct JpegInfo { pub width: u32, pub height: u32, pub components: u32, pub adobe: bool }
pub struct PixelSize { pub width: u32, pub height: u32 }
pub fn jpeg_info(b: &[u8]) -> Option<JpegInfo>
pub fn image_pixel_size(b: &[u8]) -> Option<PixelSize>                              // JPEG, PNG, GIF, BMP by the first bytes
pub fn decode_png | decode_gif | decode_bmp | decode_tiff | decode_raster(b: &[u8], limits: &ImageLimits) -> Result<Rgba, ImageError>
pub enum ImageError { Unsupported, Malformed(&'static str), Limit(LimitKind) }
pub struct ImageLimits { max_px: u64, max_dim: u32, max_inflated: u64, max_tag_values: u64 }   // NATIVE, WASM32, DEFAULT
pub fn clear_caches()                                                               // no-op: the crate has no cache
```

- `Unsupported` is the TS `null` (another format, a variant it does not decode, a damaged zlib stream); `Malformed` is what makes
  the TS **throw** (a `DataView` read past the end, a negative array length); `Limit` has no TS counterpart. For the PDF all three
  mean "the image is left out" (`pdf.ts:87` catches whatever the decoder throws); `decode_raster` goes on to the next format only
  after `Unsupported`, as the `??` chain does (a throw ends it).
- The `JpegInfo` and `PixelSize` fields are `u32` (the TS numbers: a JPEG side is 16 bits, a PNG side 32, a BMP side up to 2^31).

## Decisions outside the plan (hybrid mode: all declared)

| # | Decision | Why |
|---|---|---|
| D1 | The result is `Result<Rgba, ImageError>` with the three classes above, not `Option<Rgba>`. | The TS distinguishes `null` from a throw in the `??` chain; the writer reports the limit. |
| D2 | `ImageLimits { max_px, max_dim, max_inflated, max_tag_values }`; NATIVE 120 Mpx (the plan's 1.2·10⁸), 131,072 per side, 1 GiB of intermediate data, 2^21 TIFF tag values; **WASM32 (phone baseline) 40 Mpx** (160 MB of RGBA: a 600 dpi A4 scan is 35 Mpx, a phone photo 12), 256 MiB, 2^21; `DEFAULT` is WASM32 on wasm32. The plan named only `max_px`/`max_dim`. | `max_inflated` and `max_tag_values` bound the buffers and loops that are not the RGBA result. |
| D3 | **Allocation from declared sizes.** The RGBA buffer is the one allocation whose size comes from the file (as in the TS) and is checked against `max_px`/`max_dim` first. Every intermediate buffer is bounded by the bytes that exist (PNG rows are read from the inflated data, GIF/LZW from the blocks) or by `max_inflated` (TIFF raw samples, YCbCr blocks); the LZW and PackBits outputs are only as long as the part that is used, the rest is virtual zeros. A zlib stream is inflated with the size the header needs as its `declared` output (never preallocated from it). | "No allocation from declared sizes". A header declaring 10^10 pixels costs < 64 KiB of peak (test `m1_declared_sizes_are_not_allocated`). |
| D4 | **No ratio limit.** A DEFLATE stream cannot expand more than 1032:1, and the inflate stops at what the header needs, so a ratio limit would bound nothing beyond `max_inflated`. (The 1100 ratio of the Slides package reader is for zip entries.) | The brief listed "decompression ratio"; it is implied by `max_inflated` + the declared-size inflate. |
| D5 | PNG: the size limit is checked **after** the inflate, so a damaged stream is `Unsupported` (TS `null`) whatever the header declares; GIF, BMP and TIFF check before they allocate (the TS allocates the RGBA buffer first there). | Error-class parity (found by the differential, round 1). |
| D6 | PNG/TIFF inflate: `inflate_zlib(z, Some(needed), max_inflated)` (output cut at what is used). It refuses a **stored block that overruns** the declared size, which fflate without a size accepts; that refusal is retried once without the size, up to `max_inflated`. The TS result is then exact except for streams that inflate beyond `max_inflated`. | Found by the differential (45 mutated PNGs). |
| D7 | **TIFF fractional tags.** A RATIONAL (fractional) value in the first value of a size or count tag (`256 257 258 259 262 277 278 284 317 322 323`, the two values of `530`) is `Unsupported`; the TS computes with the fraction (a width of 7.5 gives a "7.5 x 8" image). No real file has it. 5 corpus cases, all accepted by the replay under the name `D-FRACTION` and recognised by an independent reader of the IFD. | The Rust image has integer sides; the alternative (emulating fractional loop bounds) has no user. |
| D8 | **TIFF tags.** Every tag is read as the TS does (a read past the end throws, a byte tag past the end is `undefined`), but only the 18 tags the decoder uses keep their values, and those are limited to `max_tag_values` together (the TS keeps up to 2^20 per tag, for any number of tags: 65,535 × 2^20 steps). | Time and memory bound. |
| D9 | The buffers the TS allocates from file sizes and that can fail there with a `RangeError` (the raw samples `rowBytes * height`, the LZW/PackBits `new Uint8Array(expected)`, the YCbCr blocks) are `Limit(Inflated)` above `max_inflated`. The YCbCr loops over a block are clamped to the image (the TS loops `sh * sv` per block and `continue`s: same pixels, no 4·10⁹-iteration loop). | Limits. |
| D10 | **LZW**: the dictionary is (prefix index, last byte) pairs capped at 4,096 entries (`lzw.rs`, shared by GIF and TIFF). The TS copies each entry from its predecessor and never stops adding (quadratic in time and memory on a hostile stream); a code has at most 12 bits, so no entry beyond 4,096 can be addressed: same output. Entries of a code size above 8 (GIF, `1 << minCode` up to 2,048) hold their value modulo 256, as the `Uint8Array` the TS writes them to. | Memory and time bound, same results (differential). |
| D11 | **PNG with a cut-off `IHDR`** (the file ends inside it, after an `IDAT` that came first): the TS runs NaN arithmetic (`depth` is `undefined`) and returns garbage; here it is `Malformed` once the data has inflated (before that the stream decides, as in the TS). | Not worth emulating; hostile only. |
| D12 | `JpegInfo.components` and the sides are `u32`; `imagePixelSize` of a BMP is `Math.abs` of the int32 (2^31 for the minimum) as `u32`. | The TS numbers. |
| D13 | The decoder tests use a generated pack committed with the capture (`tests/corpus/generated.pack`, 885 KB) instead of mirroring the generators in Rust: the bytes are the ones the TS saw by construction. The mutator is mirrored (20 lines). | One source of truth for the corpus; the capture script is deterministic (two runs give the same bytes on this machine; the pack holds Node's zlib output, so a recapture on another platform is a new pack and capture). |

### Arithmetic semantics (every place where a Rust integer or float operation stands for a JS one)

No decision changes a result of the TS on the corpus; these are the translations, all covered by the differential:

- `x & 0xff` of unfiltered PNG bytes and of the TIFF predictor: `u8::wrapping_add` (JS adds in double and masks).
- Typed-array stores of computed samples (`out[k] = Math.round(...)`): `uo_jsnum::conv::to_uint8` (wraps modulo 256; NaN → 0), as the
  TS stores `undefined`/`NaN` as 0. `Math.round`, `Math.max`, `Math.min` are `uo_jsnum::math` (ties towards +∞, NaN wins).
- `undefined` reads: a read past the end of a `Uint8Array` is `undefined`: 0 under `|`, `<<`, `>>`; `NaN` in arithmetic (stored as 0);
  and `false` under `===` (the `tRNS` key at 8 bits). The decoders keep that distinction per sample (`at` returns `Option`, `at0`
  is the bit-operation read) instead of padding with zeros, because the palette lookup and the transparency key tell them apart.
- Shift counts are taken modulo 32 (`1 << depth` for the PNG depths 32, 255 and the TIFF `BitsPerSample`: `wrapping_shl`, then
  `to_int32` of `(1 << n) - 1` as a double, so `1 << 31` is negative as in JS); `bit >> 3` goes through `to_int32`.
- `(data[i++] << 24) >> 24` of PackBits is `as u8 as i8`; the BMP shift `8 - bpp - (bit & 7)` can be negative and is masked to 5 bits.
- BMP `chan`: `(v & m) >>> shift` is `((v & m) >> shift)` on `u32`; `2 ** bits - 1` is `u32` (a double for 32 bits); the division
  and `Math.round` are f64.
- TIFF `YCbCr`: the f64 expression order of `toRgb` is kept (`Y - lb * B - lr * R`, then `/ lg`); Rust does not fuse multiply-adds.
- Dimensions and offsets are `u64`/`u128` where the TS uses doubles (no wrap): products of three 32-bit tag values cannot overflow.

## Parity (continuous verification)

`tools/capture-image.mjs` replays the frozen TS (`UO_TS_ROOT`, Node 24.15.0 / V8 13.6) over the corpus below and writes
`tests/captures/image.capture` (digests of the RGBA bytes and the header fields) and `tests/corpus/generated.pack`;
`tests/image_parity.rs` replays them in three tests (oracle, generated, mutated). The TS runs with the global `Uint8Array`
limited (64 MiB; 4 MiB for the mutations) and the replay uses the same numbers as limits, so "the TS tried to allocate too much"
and "limit" are both `E`.

| Corpus | Images | Facets compared (`jpeg`, `size`, `png`, `gif`, `bmp`, `tiff`) | Result |
|---|---|---|---|
| oracle: every distinct image of the 14,550 tracked files (2,742 oracle docx and the sheet/slides/site samples, plus the tracked image files): 92 PNG, 9 GIF, 26 JPEG, 29 other | 156 | 1,092 | 1,092 exact |
| generated (`gen/…`): 482 PNG (every type × depth × Adam7 × sizes × `tRNS`/`PLTE` variants × short data, plus the depths and types the specification forbids), 134 GIF (min code 1–8 and 11, interlace, transparency, tables, offsets, crops, random streams), 109 BMP (1–32 bpp, bottom-up, BITFIELDS, V3/V4/V5), 615 TIFF (both byte orders; gray, RGB, RGBA, 16 bit, 1/2/4 bit, palette, CMYK, YCbCr 1×1…4×4; strips and tiles; none, LZW, PackBits, deflate; predictor 2) and 23 JPEG header and size cases | 1,383 | 9,680 | 9,679 exact + 1 accepted (D7) |
| mutated (`mut/…`): five deterministic mutations (truncation, bit flips in the header, bit flips anywhere, random bytes, hostile 4-byte header fields) of every image ≤ 200 KB and ≤ 128 Kpx | 7,410 | 51,866 | 51,862 exact + 4 accepted (D7) |
| **total** | **8,949** | **62,638** | **0 mismatches; 5 accepted (D7)** |

`decodeRaster` is the `??` chain of the four decoders: the replay checks `decode_raster` against the chain of the captured facets
(8,949 images). Outcome classes of the 35,796 decoder facets: 5,241 `K` (decoded; the pixel digest equal), 1,288 `E` (the TS threw or
over-allocated: `Malformed`/`Limit` here), the rest `N`. The differential found four defects of the port in its first runs
(see Process metrics: 54 → 6 → 0 mismatches), all of the kind "an error class or a boundary the plan did not describe".

## Memory (measured; `tests/memory.rs`, `uo-alloc` counting allocator, release numbers)

| Check | Result |
|---|---|
| M1 no leak per operation | every operation (`jpeg_info`, `image_pixel_size`, the five decoders) on all 1,383 images of the pack and on 5 mutations of every fourth: live bytes return exactly; the decoded image is the only thing kept (`live_delta == data.capacity()`); error paths (limit, damaged data) keep nothing |
| declared sizes (D3) | a PNG, BMP, GIF and TIFF whose header declares 100,000 × 100,000 pixels (or 65,535²): `Limit`, **peak < 64 KiB**, nothing alive |
| M2 soak | 2,000 iterations over six images (PNG, GIF, BMP, 2 TIFF variants), sampled every 50: flat, **tolerance 0 B** |
| M3 budgets (allocated = peak above the start) | PNG RGBA8 1 Mpx 8.0 MiB / 4 Mpx 32.0 MiB (the RGBA buffer + the inflated rows: 2.00 ×); PNG RGB16 10.0 / 40.1 MiB (2.50 ×); PNG palette8 5.0 / 20.0 MiB (1.25 ×); BMP24 4.0 / 16.0 MiB (1.00 ×); GIF 6.1 / 24.5 MiB (1.53 ×: RGBA + indices + the copied blocks); TIFF RGB8 7.0 / 28.0 MiB (1.75 ×). Asserted: peak ≤ RGBA + the intermediate samples + 1 MiB. Time (release, Windows x64): 4 Mpx PNG RGBA8 93 ms, RGB16 159 ms, palette 81 ms, BMP 9 ms, GIF 60 ms, TIFF 53 ms |
| M4 boundaries | none: the crate has no handle; the PDF writer owns the boundary |
| M5 caches | none (`clear_caches` is a no-op; M1 and the soak run without calling it). The only buffers that outlive a call are the ones returned |
| M6 | no `unsafe` (`#![forbid(unsafe_code)]`), no Miri needed |

The phone baseline: the WASM32 limits above keep a decode at ≤ 40 Mpx × 4 B = 160 MB of RGBA plus at most 2.5 × that for the
intermediate rows in the worst PNG (RGB16: 1.5 × the RGBA), i.e. ≈ 400 MB peak at the limit; a 12 Mpx phone photo peaks at ≈ 120 MB
(PNG RGB16) and a JPEG is never decoded here. The embedding keeps the RGBA only until the PDF page that uses it is built.

## Security (no panic, limits, fuzz)

- `#![deny(clippy::indexing_slicing)]` on the library (the two functions that index have the bound in the loop and say so in an
  `#[expect]`), checked arithmetic through `u64`/`u128`, `try_reserve_exact` for every buffer: an allocation the allocator
  refuses is `Limit(Allocation)`, not an abort.
- `tests/nopanic.rs` (release, 0.4 s; 20 s with 400 rounds): every decoder over 8 mutations (default; `UO_IMAGE_NOPANIC_ROUNDS`) of
  each of the 1,383 images, **553,200 mutated images in the 400-round run, no panic**; every prefix of the small images; 24,000
  random byte strings behind the six signatures; an IHDR with every extreme value (7 × 7 sides × 7 depths × 7 types × 3
  interlace); every single-bit flip of the first 160 bytes of seven images; zero, one and maximum limits.
- Fuzz target `image_decode` (`core/fuzz/src/image.rs`, selector byte: tight limits, decoder; invariants: the RGBA buffer is exactly
  `w × h × 4` within the limits, `decode_raster` is the chain, `jpeg_info` and `image_pixel_size` agree), seeded with the 1,357
  small images of the pack: **stable driver, overflow checks and debug assertions on: 3.1 M executions (60 s) and 19.1 M (300 s, seed
  777, 16 KiB inputs), 0 crashes, slowest execution 232 ms.** libFuzzer runs it in CI with the other targets (the job takes every
  file of `fuzz_targets/`).
- Risks of the TS that the port removes: a PNG/GIF/BMP/TIFF header that declares gigapixels (`new Uint8Array(w * h * 4)` and the
  loop over it), the quadratic LZW dictionary (D10), 65,535 TIFF entries × 2^20 values (D8), a YCbCr block loop of 4·10⁹ steps (D9),
  an inflate with no output cap (D3, D6).

## Not done / open

- The converter's variant (`Raster.cs`, Slides `raster.ts`) is not ported (PQ3: with the converter).
- Real-Safari/WebKit runs of the wasm build: the crate builds for `wasm32-unknown-unknown` (clippy `-D warnings` clean) but has no
  export in `uo-wasm` yet (the PDF writer calls it natively inside the same module); the memory tests run natively only.
- The decoders are straightforward ports with one fast path (8-bit PNG rows with every sample, no per-pixel `Option`); no further
  optimisation was done (4 Mpx PNG in 93 ms). Stage-5 candidates: a streaming unfilter that frees the inflated rows as it goes
  (PNG peak 2.0 × → 1.1 ×), `tRNS` and palette fast paths.
- The pack and the capture are 2.1 MB together (the pack is Node's zlib output: recapturing on another platform changes it).

## Process metrics

| Stage | Agent (model) | Time | Tool calls | Rounds / rework |
|---|---|---|---|---|
| 3 + 4 (port, differential, memory, no-panic, fuzz target, record) | porter (Sonnet, hybrid mode) | ≈ 65 min wall (08:50–09:55), of which ≈ 20 min reading the TS, the plans and the foundation crates | ≈ 125 (estimate) | 0 rounds from a verifier. The differential I built alongside found **4 defects of my own port** in three runs (54 → 6 → 0 mismatches; all "plan gaps" of the kind the hybrid mode expects): (1) the inflate with a declared size refuses a stored block that overruns it, which the TS accepts (45 mutated PNGs; D6); (2) the PNG size limit was checked before the inflate, so a damaged stream became `E` instead of `N` (D5); (3) the PNG IHDR-cut-off error was raised at parse time instead of after the inflate (D11); (4) the TIFF deflate output was capped at `expected` where the TS uses the whole stream (1 mutated YCbCr strip). Also: the TS corpus run hung in my own generator (a BMP with a negative width allocated 4·10⁹ bytes of content: fixed in the script), and the mutated oracle screenshots needed a 4 MiB allocation limit in the TS replay because fflate's doubling buffer falls on either side of any limit (the mutations skip images above 128 Kpx). 13 decisions declared (D1–D13 above). The shared workspace did not build for ≈ 50 min (another agent's half-written `uo-docx` manifest): the work ran in a private copy of the five crates it needs, and the final fmt/clippy/tests/wasm checks ran in the real workspace. 3 wrong expectations of mine in the hand-made tests (a GIF code width after the dictionary reaches 8 entries, the `jpeg_info` truncation length, the Adam7 expected pixel order) fixed against the TS, not the code; 2 clippy findings in the tests |
