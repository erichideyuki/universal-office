# uo-jsnum — JavaScript numerics

Record of the rust-port workflow. Status: **stage 1 plan, revised after the analysis skeptic; gate 1
pending** (planner and skeptic, 2026-11-01). Delivered in two increments (A now, B before
uo-pdf/uo-docx/uo-charts; see "Increments").
New shared crate, stage 1 of the dependency order: every engine crate (`uo-fonts`,
`uo-drawingml`, `uo-docx`, `uo-docx-layout`, `uo-pdf`, `uo-charts`, Slides, Sheet calc) calls it
instead of `f64` methods of `std`, so that native, wasm and .NET give the same bits as the frozen
TS engine running in Node.

## Why (evidence)

- The uo-drawingml reuse probe (400k samples per function, Node 24.15 / V8 13.6 on Windows x64)
  measured bit differences against `Math.*`: Rust std (UCRT) tan 15,809, atan2 70,829, exp
  37,669, sin 9,296, log 7,789; the `libm` crate 0.2.16 sin 3,623, cos 3,633, log 732, pow
  25,143/39,129, hypot 143,219. Std math is the platform libm (UCRT natively, compiler-builtins
  libm in wasm), so wasm and .NET would also disagree with each other.
- Re-checked for this plan (`%TEMP%/uo-mathprobe`, `/tmp/jsnumscratch`, V8 sources at tag
  `13.6.233.17`, Node `v24.15.0` sources):
  - **Node 24 uses V8's own fdlibm 5.3 port for every `Math` function except `pow`.** V8 GN builds
    define `V8_USE_LIBM_TRIG_FUNCTIONS` when `is_clang` (`gni/v8.gni`) and then call glibc's
    `s_sin.c` for sin/cos (runtime flag `--use-libm-trig-functions`, default true), but Node builds
    V8 with gyp (`tools/v8_gypfiles/v8.gyp`, `features.gypi`), which never sets the define nor
    compiles `third_party/glibc`. So Node takes `fdlibm_sin/cos` = `base::ieee754::sin/cos`.
    This also explains the `libm` crate differences: FreeBSD's `k_cos.c` (Evans' rewrite) is not
    fdlibm 5.3's `qx` variant that V8 keeps (`ieee754.cc:305`).
  - **`pow` is not fdlibm in Node 24**: `v8::internal::math::pow` (`src/numbers/ieee754.cc`)
    calls `std::pow` after the ES special cases when `--use-std-math-pow` (default true); with
    `--no-use-std-math-pow` it calls `base::ieee754::legacy::pow` (fdlibm `e_pow.c`, V8 variant)
    with no extra special cases.
  - Measured here, same machine: Node default `pow` vs Node `--no-use-std-math-pow`: **39,564 /
    400k** random pairs differ (x ∈ [0,100), y ∈ [−5,5)); for the sRGB/gamma exponents the share is
    ~10% each (y = 2.4: 4,253/44,444; 1/2.4: 3,865; 2.2: 4,314; 3: 4,478; 10: 4,303; y = 2 and 0.5:
    0, they are special-cased). `libm::pow` vs V8 legacy pow: 17,755 / 400k (not the same code).
    Rust `f64::powf` (system `ucrtbase.dll`) vs Node's `std::pow`: 146 / 400k — even "Windows UCRT"
    is not one reference.
  - **Chromium 152 is not Node 24.** Digest of 400k samples per function, identical seeds
    (`/tmp/trig.js`), Node 24.15 vs the Chromium 152 of the desktop app (Electron, Windows x64):
    sin, cos, tan, exp, log, atan2, cbrt, log10 **differ**; pow and hypot are equal. V8 replaced
    fdlibm by LLVM libc's math in May 2026 (commits `08be4445f6` log/log1p/log2/log10, `cc1a6fcbda`
    tan, `43f04a52f2` atan/atan2, exp/expm1, acos/asin, sin/cos, tanh; `716d3e3363` legacy pow;
    `8787f0842a` removed glibc sin/cos). LLVM libc is correctly rounded for log*, tan, exp per those
    commits; not yet for atan/atan2/pow.

## Scope

- **Covered**: the JavaScript numeric semantics the engines use, as V8 13.6.233.17 (Node 24.15.0)
  computes them: the `Math` functions in use, `**`, ToNumber of strings, `parseInt`, `parseFloat`,
  Number → string (`String(n)`, template literals, `toFixed`, `toPrecision`, `toExponential`,
  `toString(radix)` of integers), ToInt32/ToUint32 and the typed-array store conversions, the JS whitespace
  set (`trim`, `split(/\s+/)`, StringToNumber), number truthiness.
- **TS covered**: none of the TS is ported here; the reference is V8 (behaviour of the
  operations the TS calls). The users are listed below so the crate holds exactly what they need.
- **C# (type guide only)**: `dotnet/src/UniversalOffice.Core/JsNumber.cs` (`ToString`,
  `ToFixed`, `PrecisionDigits`, `Round`, `ParseFloat`, `ParseInt`), copied in 3 places
  (drawingml reuse report). Known C# divergences, not to be copied: `Round(-0.4)` and
  `Round(-0.5)` return +0 (JS −0); `ParseInt` with radix ≠ 10 accumulates in double (inexact
  above 2^53; V8 rounds exactly for power-of-two radices) and only skips `0x` for radix 16;
  `Math.Sin/Pow/Exp/Log` are used directly in `Pptx/Read.cs`, `Pptx/Geometry.cs`,
  `Pptx/Export/PptxPdf.cs`, `Xlsx/Format.cs`, `Core/Pdf/PdfDocument.cs` (platform libm).
- **Not covered (owner)**: `Math.random` (calc `RAND`, editor shell: never part of parity);
  `BigInt` (calc `engineering.ts`); `Intl`/`toLocaleString` (calc i18n, converter
  `chart-engine.ts:450` `toLocaleString('pt-BR')`: the converter plan decides; ICU rounds the
  shortest decimal, not the exact binary value); `localeCompare` (fonts 1, editor 6); `Date`;
  the correctly-rounded double-double functions of `sheet/packages/calc/src/functions/libm.ts`
  (calc logic built on these primitives, goes with the Sheet calc crate); `acosh`/`asinh`
  (unused today; add with Sheet calc if its `ACOSH`/`ASINH` call `Math.*`).

### Inventory (engine packages, non-test `.ts`)

Packages counted as engine: write `docx-core`, `docx-layout`, `compare`; shared `charts`,
`drawingml`, `fonts`; sheet `sheet-core`, `calc`; slides `slides-core`; converter `*`. The write
`editor` is mostly shell (its model ops move to `uo-docx`; its counts are given apart).
`viewer`, `sheet`, `slides` (UI) and `shared/ui` stay in TS.

| Function | Engine users (occurrences) | Editor |
|---|---|---|
| `sin` / `cos` | slides-core 56/57, docx-layout 31/29, charts 31/31, drawingml 17/17, slides-processing 10/10, sheet-core 7/7, converter 5/5, docx-core 3/3, sheets-processing 2/2, calc 1/0 | 25/24 |
| `tan` | docx-layout 5 (geometry, canvas-picture, pdf-picture), drawingml 4 (preset-engine, light3d, effects), slides-core 3, docx-core 1 (drawing), sheet-core 1, converter 1 | 2 |
| `asin` / `acos` / `atan` | slides-core anim-formula/render (1/2/1), drawingml light3d (atan 1) | — |
| `atan2` | slides-core 11, drawingml 6 (preset-engine, bevel), docx-core 3 (drawing), sheet-core 2, calc 1 | 2 |
| `exp` | calc 36, drawingml 3 (raster, adjust), charts 2 (trend), slides-core 2, docx-layout 1 (geometry) | — |
| `expm1`, `log1p`, `atanh` | calc only (4, 8, 1) | — |
| `log` | calc 34, charts 13 (trend, scale), drawingml 2 (adjust), slides-core 1 | — |
| `log2` | fonts 1 (subset), converter 1 (pdf-writer subset), calc 4 | — |
| `log10` | charts 3 (scale, numfmt), sheets-processing 3 (format), calc 3 | — |
| `sinh` / `cosh` / `tanh` | calc 4/3/0 (complex), slides-core anim-formula 1/1/1 | — |
| `cbrt` | charts 1 (chartex) | — |
| `pow` and `**` | calc 33+20, charts 9+11, sheets-processing 9, slides-core 9+8, drawingml 7 (color, bevel, adjust), docx-core 3 (table-formula), docx-layout 1+4, fonts 0+1, converter 0+1 | 1+4 |
| `hypot` | slides-core 29, docx-layout 8, drawingml 8, charts 4, sheet-core 1, calc 1 | 3 |
| `sqrt` | calc 49, slides-core 14, docx-layout 4, drawingml 4, charts 2, docx-core 2, slides-processing 1 | 1 |
| `round` | slides-core 196, docx-core 158, docx-layout 96, sheet-core 49, charts 28, calc 18, converter 12, drawingml 8, others 9 | 382 |
| `max` / `min` | 1,601 / 1,005 in total (all packages) | incl. |
| `abs`, `floor`, `ceil`, `trunc` | 567, 279, 122, 158 (trunc: calc 154) | incl. |
| `sign` | slides-core 7, calc 4, docx-core 3, drawingml 3, docx-layout 2 | 1 |
| `imul` | docx-core 7 (source, docx-writer hashes), slides-core 4, compare 1 | — |
| `clz32` | docx-layout 2 (images) | — |
| `fround` | 0 (but every `Float32Array`/`setFloat32` store is a fround) | — |
| `toFixed` | slides-core 12, docx-core 8 (export/model `twipsToPt`, odf/write-odt, table-formula), sheets-processing 8, sheet-core 6, converter 3, charts 2, slides-processing 1 | 27 |
| `toPrecision` | calc 8, charts 6 (numfmt `(15)`, scale `(12)`, chartex `(2)`), sheet-core 1, sheets-processing 1 | — |
| `toExponential` | calc 3, sheet-core 1 (all `(14)`) | — |
| `toString(radix)` | `(16)` 79 (gids, colours, hex escapes), `(36)` 4 (slides-core emfplus key), `(radix)` calc engineering 2 | incl. |
| Number → string (`String(n)`, `${n}`) | pdf-writer `fmt` (`document.ts:90`, `String(round(x·1000)/1000)`), XML/ODF/HTML writers, keys | many |
| `parseInt` | 615 calls: radix 10 (539), 16 (124), 36 (docx-layout `fonts.ts:128`), variable (calc) | 42 |
| `parseFloat` | slides-core 25, docx-core 29, sheet-core 15, sheets-processing 14, charts 6, others 3 | 32 |
| `Number(str)` | calc 35, slides-core 9, docx-core 6, drawingml 5 (custGeom tokens), charts 4, sheet-core 3, fonts 1, sheets-processing 1 | 86 |
| `\| 0`, `>>> 0`, `~~`, shifts | `\|0` 45 real, `>>>0` 44, `~~` 5, shifts 911 (ToInt32 on doubles) | incl. |
| `%` on doubles | 357 binary uses (calc 79, slides-core 67, docx-layout 52, …) | 38 |
| `Uint8ClampedArray` | drawingml 8, docx-layout 4, slides-core 3 | 3 |
| `Uint8Array` / `Int32Array` / `Uint32Array` | 179 / 21 (slides-core 16, compare 4, drawingml 1) / 13 | |
| `Float32Array` / `Float64Array` | drawingml 21, slides-core 5, docx-layout 1 / slides-core 9, drawingml 4, calc 2 | |
| DataView | `get/setFloat32` docx-layout 13, slides-core 3; `Float64` sheet-core 2, calc 3; int getters/setters 512 | |
| `Number.isInteger/isFinite/isNaN` | 17 / 259 / 26 | |
| `trim()` / `split(/\s+/)` | 112 / 3 (custGeom tokenizer, attribute lists) | |
| `Int8/Int16/Uint16Array` | 0 (the ToInt8/16 conversions are still needed by DataView `set*`) | |

Planned crates and what they need *(revised after the skeptic, S10, S14; uo-drawingml A13)*:
**uo-fonts** needs nothing from uo-jsnum (no transcendentals; `ilog2`/shifts; its JS string
semantics live in its private `js` module, uo-fonts S15) and does not wait for it;
**uo-drawingml** (increment A: sin, cos, tan, atan, atan2, exp, log, pow, hypot, round, sign,
min/max, floor/ceil/trunc/abs/sqrt, `to_uint8_clamp`, `to_int32`, `fround`, `js_index`,
`to_number`, `parse_int` 16, whitespace and UTF-16 helpers); **uo-docx-layout** (sin, cos, tan,
exp, pow, hypot, clz32, round, setFloat32/getFloat32, parseInt 10/16/36, parseFloat);
**uo-pdf** (Number → string, toString(16), sin/cos); **uo-docx** (parseInt, parseFloat, toFixed,
imul, pow, atan2, round); **uo-charts** (log, log10, exp, cbrt, pow, toPrecision, toFixed);
**Slides** (all of the trig and hyperbolic functions, Int32Array); **Sheet calc**
(exp/log/log1p/expm1/atanh/sinh/cosh/pow/trunc, toPrecision, toExponential(14), Number(),
isInteger, EPSILON).

## Plan (gate 1 — pending; revised after the skeptic)

### Increments *(revised after the skeptic, S14)*

The crate is delivered in two increments, each with its own stages 3–4 and gate 2, recorded
here:

- **Increment A — now (unblocks uo-drawingml)**: `fdlibm` (all 19 functions, range reduction,
  kernels; cheap once `rem_pio2`, the kernels and `k_log1p` exist), `pow`, `hypot*`, the spec
  functions of `math`, all of `conv` (with `js_index` and `canonicalize_nan`), all of `text`
  (with `utf16_len`/`utf16_slice`), `parse::to_number`, `parse::parse_int`.
- **Increment B — before uo-pdf, uo-docx and uo-charts**: `format` (`Js` via ryu-js,
  `to_fixed`/`to_precision`/`to_exponential`, integer `to_radix_string`) and
  `parse::parse_float`.

### Crate and reuse

- New crate `core/crates/uo-jsnum`, used by every engine crate of Write, Sheet and Slides; no Word
  assumptions anywhere (it is the JS number model, nothing else). It is the only place where
  `f64` transcendental/rounding methods may be called (enforced, see "Enforcement").
- `uo-opc` has `xml/chars.rs` `js_space_len`/`is_js_space` (`pub(crate)`); the JS whitespace set
  moves to `uo_jsnum::text` and `uo-opc` depends on `uo-jsnum` after increment A passes gate 2
  (G7). Until then the two copies must stay identical (same set, verified over every code point
  by the skeptic, S9: U+0009–000D, 0020, 00A0, 1680, 2000–200A, 2028, 2029, 202F, 205F, 3000,
  FEFF; not U+0085, not U+180E).
- `uo-testkit` gets the `@platform` metadata (below); it still never depends on `uo-jsnum`.
- `#![forbid(unsafe_code)]`. **Not `no_std`**: in Rust 1.97 `core` has no `f64::sqrt`, `floor`,
  `ceil`, `trunc` (checked: `rustc --crate-type=rlib` of a `#![no_std]` lib fails on all four,
  native and wasm32), and a software sqrt would be slow; every target we ship has `std`. Revisit
  when `core_float_math` is stable.
- Dependencies: none in increment A; `ryu-js = "=1.0.3"` (`default-features = false`) in
  increment B (see "Reuse").

### Data model and memory scopes

No state, no caches, no handles. Numeric functions are pure `fn(f64…) -> f64` with no heap
allocation. String outputs allocate exactly the returned `String`; `Js` (Display) formats on the
stack. Static tables only (fdlibm `ipio2`/`PIo2`, the pow tables under G1 (a), ryu-js tables).
The bignum of the exact decimal formatter is a fixed `[u32; 84]` on the stack (2,688 bits ≥ 53 +
log2(5^1074) ≈ 2,547). `utf16_slice` returns `Cow::Borrowed` unless a surrogate pair is cut.

### Public API (closed for the port)

```rust
// ── increment A ──────────────────────────────────────────────────────────────
// uo_jsnum::math — Math.* and ** (V8 13.6.233.17 as run by Node 24.15.0 linux-x64, see G1/G2)
pub fn sin(x: f64) -> f64;   pub fn cos(x: f64) -> f64;   pub fn tan(x: f64) -> f64;
pub fn asin(x: f64) -> f64;  pub fn acos(x: f64) -> f64;  pub fn atan(x: f64) -> f64;
pub fn atan2(y: f64, x: f64) -> f64;
pub fn exp(x: f64) -> f64;   pub fn expm1(x: f64) -> f64;
pub fn log(x: f64) -> f64;   pub fn log1p(x: f64) -> f64; pub fn log2(x: f64) -> f64; pub fn log10(x: f64) -> f64;
pub fn sinh(x: f64) -> f64;  pub fn cosh(x: f64) -> f64;  pub fn tanh(x: f64) -> f64; pub fn atanh(x: f64) -> f64;
pub fn cbrt(x: f64) -> f64;
pub fn pow(x: f64, y: f64) -> f64;            // Math.pow and **; G1
pub fn hypot(a: f64, b: f64) -> f64;          // Math.hypot(a, b)       (V8 FastMathHypot, 2 args)
pub fn hypot3(a: f64, b: f64, c: f64) -> f64; // Math.hypot(a, b, c)    (FastMathHypot, 3 args)
pub fn hypot_n(v: &[f64]) -> f64;             // Math.hypot(...v): 0 → 0, 1 → |a|, 2/3 → above, >3 → Kahan
pub fn round(x: f64) -> f64;                  // half towards +∞, −0 for x ∈ [−0.5, −0]
pub fn sign(x: f64) -> f64;                   // NaN → NaN, ±0 → ±0
pub fn max(a: f64, b: f64) -> f64;            // NaN wins, +0 > −0
pub fn min(a: f64, b: f64) -> f64;            // NaN wins, −0 < +0
pub fn max_n(v: &[f64]) -> f64;               // Math.max(...v), [] → −∞; never fails (S13)
pub fn min_n(v: &[f64]) -> f64;               // Math.min(...v), [] → +∞; never fails (S13)
pub fn imul(a: f64, b: f64) -> f64;           // ToInt32 both, wrapping_mul, as f64
pub fn clz32(x: f64) -> f64;                  // ToUint32(x).leading_zeros()
pub fn fround(x: f64) -> f64;                 // x as f32 as f64 (IEEE RNE, = V8 DoubleToFloat32)
pub fn floor(x: f64) -> f64; pub fn ceil(x: f64) -> f64; pub fn trunc(x: f64) -> f64;
pub fn abs(x: f64) -> f64;   pub fn sqrt(x: f64) -> f64; // exact in IEEE; wrappers so every Math.X maps to math::X
pub fn rem(a: f64, b: f64) -> f64;            // JS %, = Rust % (fmod, exact); wrapper for uniform porting

// uo_jsnum::conv — ToIntN, typed-array stores and reads, predicates
pub fn to_int32(x: f64) -> i32;    pub fn to_uint32(x: f64) -> u32;   // |0, >>>0, shifts, Int32Array/Uint32Array
pub fn to_int16(x: f64) -> i16;    pub fn to_uint16(x: f64) -> u16;   // DataView setInt16/setUint16
pub fn to_int8(x: f64) -> i8;      pub fn to_uint8(x: f64) -> u8;     // Uint8Array, DataView setInt8/setUint8 (wraps)
pub fn to_uint8_clamp(x: f64) -> u8;                                  // Uint8ClampedArray (round half to even)
pub fn js_index(i: f64, len: usize) -> Option<usize>;                 // ta[i] with f64 i (uo-drawingml A6): None for NaN, fractional,
                                                                      // negative, ≥ len; −0 → Some(0). Read → NaN, write → no-op
pub fn canonicalize_nan(x: f64) -> f64;                               // any NaN → 0x7ff8000000000000 (S5)
pub fn truthy(x: f64) -> bool;                                        // false for ±0 and NaN (`x || d`, `if (x)`)
pub fn is_integer(x: f64) -> bool; pub fn is_safe_integer(x: f64) -> bool;

// uo_jsnum::text — the JS whitespace set (StrWhiteSpaceChar = WhiteSpace ∪ LineTerminator = \s)
pub fn is_js_space(c: char) -> bool;
pub fn js_space_len(b: &[u8], i: usize) -> usize;   // UTF-8 bytes of a JS space at b[i], 0 if none
pub fn trim(s: &str) -> &str; pub fn trim_start(s: &str) -> &str; pub fn trim_end(s: &str) -> &str;
pub fn split_js_space(s: &str) -> SplitJsSpace<'_>; // s.split(/\s+/): leading/trailing "" kept, "" → [""]
pub fn utf16_len(s: &str) -> usize;                 // s.length (code units)          (S10)
pub fn utf16_slice(s: &str, start: i64, end: i64) -> Cow<'_, str>;
    // s.slice(start, end) in code units with JS clamping (negative from the end); a cut surrogate
    // half becomes U+FFFD. Exact for parsing and whitespace tests (every numeric parser treats any
    // non-ASCII non-space char as junk), not for re-emitting text.

// uo_jsnum::parse — string → number
pub fn to_number(s: &str) -> f64;               // Number(s), unary +s (StringToNumber)
pub fn parse_int(s: &str, radix: i32) -> f64;   // radix after ToInt32; 0 = undefined (auto 0x)

// ── increment B ──────────────────────────────────────────────────────────────
pub fn parse_float(s: &str) -> f64;             // uo_jsnum::parse
// uo_jsnum::format — number → string
pub struct Js(pub f64);                          // impl Display = Number::toString(x) (shortest)
pub fn to_js_string(x: f64) -> String;           // = Js(x).to_string()
pub struct RangeError;                           // argument out of range (JS throws)
pub fn to_fixed(x: f64, digits: u32) -> Result<String, RangeError>;        // 0..=100
pub fn to_precision(x: f64, precision: u32) -> Result<String, RangeError>; // 1..=100
pub fn to_exponential(x: f64, digits: u32) -> Result<String, RangeError>;  // 0..=100 (only the explicit-digits form is used)
pub fn to_radix_string(x: f64, radix: u32) -> Result<String, RangeError>;
    // (revised after the skeptic, S8) integral |x| < 2^53 and radix 2..=36 only, else Err;
    // every engine caller formats integers; the fraction loop is ported when a caller needs it
```

Callers convert non-string operands of `Number(…)` themselves (`undefined` → NaN, `null` → 0,
booleans), at the call site of the engine crate. Constants: only the six that exist in JS
(`PI`, `E`, `LN_2`, `LN_10`, `SQRT_2`, `FRAC_1_SQRT_2` of `core::f64::consts`) may be used, and a
test asserts their bits equal `Math.PI`, `E`, `LN2`, `LN10`, `SQRT2`, `SQRT1_2`; the others
(`FRAC_PI_3`, `TAU`, `LOG2_E`…) are correctly rounded values where the TS computes an expression
(`Math.PI / 3`), so they are banned (CI grep, see "Enforcement"; uo-drawingml G15b).

`max_n`/`min_n`/`hypot_n` never fail, while the TS throws `RangeError` when a spread passes more
than ~120k–200k arguments (measured by the skeptic; charts `render.ts:587–733` spreads
document-sized arrays): an accepted divergence; capture generators record the TS exception as
an error outcome, never as a value *(revised after the skeptic, S13)*.

### Source to port, per function (closed scope)

All V8 code from tag `13.6.233.17` (the V8 of Node `v24.15.0`, `process.versions.v8 =
13.6.233.17-node.48`). The skeptic compiled this `ieee754.cc` standalone with `g++ -O2
-ffp-contract=off` and it equals Node 24.15 x64 `Math.*` bit for bit on 1M inputs per function,
and `legacy::pow` equals `node --no-use-std-math-pow` (S1): the port is a translation against an
exact oracle. Port **mechanically**: same statements, same order of operations, same constants
(written as `f64::from_bits(0x…)` from the hex comments, with a test that each also equals the
decimal literal), C `uint32_t` arithmetic as `wrapping_*`, `GET_HIGH_WORD` etc. as small inline
helpers in `fdlibm/mod.rs`.

| Rust | Source | Lines (13.6.233.17) | Increment |
|---|---|---|---|
| `fdlibm::rem_pio2`, `kernel_rem_pio2` | `src/base/ieee754.cc` `__ieee754_rem_pio2`, `__kernel_rem_pio2` | 118, 443 | A |
| `kernel_cos`, `kernel_sin`, `kernel_tan` | `__kernel_cos` (fdlibm 5.3 `qx` variant), `__kernel_sin`, `__kernel_tan` | 305, 673, 733 | A |
| `acos`, `asin`, `atan`, `atan2` | same names | 862, 991, 1116, 1228 | A |
| `cos`, `sin`, `tan` | `fdlibm_cos` / `cos`, `fdlibm_sin` / `sin`, `tan` (the non-glibc branch) | 1352, 2448, 2510 | A |
| `exp`, `expm1` | `exp`, `expm1` | 1447, 2213 | A |
| `log`, `log1p`, `log2`, `log10` (+ `k_log1p`) | same | 1638, 1783, 1951, 1973, 2079 | A |
| `atanh`, `cosh`, `sinh`, `tanh` | same | 1557, 2554, 2930, 2986 | A |
| `cbrt` | `cbrt` | 2333 | A |
| `pow`, G1 (a) — recommended | wrapper = the `use_std_math_pow` branch of `v8::internal::math::pow` (`src/numbers/ieee754.cc`): y NaN → NaN; y ±∞ and \|x\| = 1 → NaN; x NaN → quiet NaN; y = 2 → `x * x`; y = 0.5 → +∞ if x is ±∞ else `sqrt(x + 0)`; else the core. Core = ARM optimized-routines `math/pow.c` + `pow_log_data.c` + `exp_data.c` + `math_config.h` (MIT OR Apache-2.0 WITH LLVM-exception), **non-FMA branch** (`__FP_FAST_FMA` undefined, `TOINT_INTRINSICS` 0), at the commit whose algorithm and tables equal glibc 2.39 `sysdeps/ieee754/dbl-64/e_pow.c` (diff recorded; the glibc copy, LGPL, is never the source) | — | A |
| `pow`, G1 (b) — fallback | `legacy::pow` of `src/base/ieee754.cc`, called directly (with the flag, `math::pow` goes straight to it, none of the wrapper cases) | 2646 | A |
| `hypot`, `hypot3`, `hypot_n` | `src/builtins/math.tq` `FastMathHypot` (2 and 3 arguments; note the 3-argument compensation `(a²+b²) − a² − b²`) and `MathHypot` slow path (Kahan, >3 args); Infinity checked before NaN | 399–510 | A |
| `round` | spec (`Float64Round`: `r = ceil(x); if r − 0.5 > x { r − 1 } else { r }`) | — | A |
| `sign`, `max`, `min`, `max_n`, `min_n`, `imul`, `clz32`, `fround`, `conv::*`, `text::*` | ECMA-262 (exactly defined; no implementation choice) | — | A |
| `to_number` | ECMA-262 grammar (StringNumericLiteral), validated by hand, then `core::str::parse::<f64>` on the validated decimal slice (correctly rounded, like V8's `Strtod`); `0x/0o/0b` with round-half-even and sticky bit like V8 `InternalStringToIntDouble` | `src/numbers/conversions.cc` 192 | A |
| `parse_int` | `NumberParseIntHelper` (`conversions.cc` 487–700): radix 10 → decimal digits through the same correctly-rounded parse; power-of-two radices → `InternalStringToIntDouble<bits>`; other radices → `HandleGenericCase` (597) verbatim | — | A |
| `parse_float` | ECMA-262 StrDecimalLiteral prefix, same parse | — | B |
| `Js`/`to_js_string` | `ryu-js` `Buffer::format` (G4; byte-identical to `String(x)` on 3M values, S6) | — | B |
| `to_fixed`, `to_precision`, `to_exponential` | ECMA-262: exact decimal expansion of the binary value (fixed stack bignum), round half up in magnitude ("pick the larger n"), sign prefixed separately; `to_fixed` with \|x\| ≥ 1e21 → `Js(x)`. V8 is exactly this rule (S7: 960,064 calls against a BigInt reference). Model: G13 | — | B |
| `to_radix_string` | integers only: digits of the exact integer, `-` prefix (S8) | — | B |

File layout: `src/lib.rs` (crate docs, `#![forbid(unsafe_code)]`), `src/fdlibm/{mod,rem_pio2,
kernels,trig,explog,hyper,cbrt}.rs`, `src/pow/{mod,log_data,exp_data}.rs` (G1 (a); under (b)
`src/fdlibm/pow.rs`), `src/math.rs`, `src/conv.rs`, `src/text.rs`, `src/parse.rs`,
`src/format/{mod,exact,radix}.rs` (B), `LICENSE-V8` (V8 BSD-3-Clause text and the Sun notice),
`LICENSE-optimized-routines` (G1 (a)), `js/capture-jsnum.mjs`, `js/platform-probe.mjs`,
`tests/captures/jsnum.capture`, `tests/{differential,special,strings,constants,memory}.rs`.

### Reuse (crate vs own port)

| Candidate | License | Parity evidence | wasm | Decision |
|---|---|---|---|---|
| `libm` 0.2.16 | MIT | FreeBSD msun (Evans variants): sin 3,623, cos 3,633, log 732 / 400k vs Node; pow vs V8 legacy 17,755; exp/tan/atan 0 | ~13.4 KB raw / 6 KB gzip for 8 functions (drawingml probe) | **No** (wrong reference); banned as an engine-crate dependency (S11) |
| own V8 fdlibm port | BSD-3-Clause (Google) + Sun notice | is the reference (S1: bit-identical on 1M per function) | estimate 25–35 KB raw for the 19 functions + tables; measured at stage 3 | **Yes** (G3 for the license) |
| own port of ARM optimized-routines `pow` (G1 (a)) | MIT OR Apache-2.0 WITH LLVM-exception | equals glibc's `pow` (same code); glibc non-FMA vs Node-Windows UCRT 769 / 1M (S2) | tables ≈ 4–5 KB (128-entry log + exp tables) | **Yes** under (a) |
| `ryu-js` 1.0.3 (Boa) | Apache-2.0 OR BSL-1.0 | byte-identical to `String(x)` on 3M values (S6) | tables ≈ 10 KB (feature `small` shrinks them; not measured); no_std, MSRV 1.71, only optional dep `no-panic` | **Yes** for shortest `toString` (G4, increment B); `format_to_fixed` not used |
| CORE-MATH (`core-math` crate 1.4 binds the C code) | MIT | correctly rounded; matches Node only where fdlibm happens to be correctly rounded (V8's own measurements: log 80.7%, tan 81.3%, atan 82.5%) | needs a C toolchain for `wasm32-unknown-unknown`; a Rust port is large | **No** for 0.10 |
| `num-traits` 0.2 | MIT/Apache | `Float::sin` etc. delegate to std or `libm` | — | **No**; banned as an engine-crate dependency (S11) |
| LLVM libc math | Apache-2.0 WITH LLVM-exception | what Chromium ≥ ~149 runs; not Node 24 | — | **No** for 0.10 |

### pow — maintainer decision (G1) *(revised after the skeptic, S2)*

Node 24's `Math.pow`/`**` result depends on the capture platform's C runtime (with the default
`--use-std-math-pow`). The planner's first draft recommended (b) on "1 ulp in ~10 % of calls";
the skeptic measured what each option does to the results users actually get, and the plan now
recommends **(a)**.

| | **(a) Linux capture + optimized-routines pow** (recommended) | **(b) `--no-use-std-math-pow` + V8 legacy pow** (fallback) |
|---|---|---|
| Reference | Node 24.15 linux-x64 in WSL Ubuntu-24.04 (glibc 2.39), non-FMA glibc path forced: `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2` | Node 24.15 x64 on any OS with the flag |
| Differs from what users run (1M pairs, 4 distributions, S2) | glibc non-FMA vs Windows UCRT (default Node, Chrome/Electron on Windows): 769 (0.08 %); glibc FMA (Linux servers, Linux Chrome on FMA CPUs) vs non-FMA: 549; glibc FMA vs UCRT: 542 (0.05 %) | flag vs default Windows Node: 62,994 (6.3 %): sRGB/gamma exponents 15,906 / 250k, x ∈ [0,100) × y ∈ [−5,5) 24,670, integer y ∈ [−20,20] 22,339 |
| Not correctly rounded (37,428 finite non-trivial pairs vs a 60-digit reference) | 41 (0.1 %) non-FMA, 36 FMA; UCRT 18 (0.05 %) | 3,070 (**8.2 %**) |
| Visible effects | none found | `Math.pow(10, n) !== Number('1e' + n)` for n ∈ {−39, −32, −29, −24, −23, −21, −20, −17, **−5, −4**, 26, 29, 34, 36, 39} (default Node: only n = 23): `String(Math.pow(10, -4))` = `"0.00009999999999999999"`, `7.5 / Math.pow(10, -5)` = `750000.0000000001`. TS sites using `Math.pow(10, e)` with negative `e`: charts `numfmt.ts:292, 824–827`, `scale.ts:17`; docx-core `table-formula.ts:183, 242`; calc `text-format.ts:315`; sheets-processing `format.ts:150–157, 420–424`. drawingml: `SRGB_LIN` changes in 33/256 entries, blackbody (`adjust.ts:52–53`) 65/668, rounded `LIN_SRGB` 0/4096 |
| Port | ARM optimized-routines `pow`, non-FMA branch, + V8's std-branch special cases (table above); **Opus** (branch selection, tables, the wrapper) | `legacy::pow` from the same `ieee754.cc`; Sonnet |
| Capture machine | every capture whose path calls a `uo-jsnum` math function runs in WSL (Node 24.15 linux-x64 installed there, G10); math-free captures may still run on Windows | any x64 machine with the flag |
| Cost | Node in WSL; the env var on every math capture; an Opus port | the visible effects above, written in `CHANGELOG.md` as a 0.10 change of results ("powers of ten and the sRGB curve now follow V8's fdlibm pow: e.g. 10^-4 prints 0.00009999999999999999"); and no later switch to correctly rounded (it would re-capture every module) |

Rejected: **(c) correctly rounded (CORE-MATH)** — parity would need a growing list of accepted
differences, the workflow forbids tolerances; **(d) Windows UCRT** — closed source; Node's
statically linked UCRT also differs from the system `ucrtbase.dll` in 146/400k.

Recommendation: **(a)**, with (b) as the documented fallback if the maintainer rejects Linux
captures. `js/platform-probe.mjs` prints one digest per function and distribution (same seeds as
the differential) and fills the platform table of this record: Node 24 win-x64 (11 functions
done, "Why"), linux-x64 with FMA masked and unmasked (needs G10), macOS arm64 (informative,
expected to differ, S3), and as a page in Chrome, Edge, Firefox, Safari (BrowserStack). Stage 3
also checks that the 19 fdlibm functions give identical digests on Node win-x64 and linux-x64
(both are baseline x64 builds without FMA contraction).

### Where captures may run *(revised after the skeptic, S3, S4)*

- **x64 only.** Node's gyp sets `-ffp-contract=off` only for s390x and ppc64 (`toolchain.gypi:307,
  328` at v24.15.0); arm64 builds compile `ieee754.cc` with the compiler default, so an arm64 Node
  is a different reference (simulated contraction on x64: sin 4,313, cos 4,284, tan 8,136, atan2
  496, exp 232, legacy pow 309 per 1M). The Rust side never contracts (rustc does not; `mul_add`
  is banned). The "Risks" item 1 holds for x64 only.
- **Node only, never a browser.** Chromium/Electron give different `Math.*` bits with any flag:
  Chromium 152 differs on 8 of 11 functions, its `legacy::pow` is LLVM libc since `716d3e3363`
  (so `--js-flags=--no-use-std-math-pow` does not give fdlibm pow), and clang-built V8 13.6
  (Chromium 136) takes glibc `s_sin.c` for sin/cos. A browser run is a reference only for
  math-free outputs.
- **Consequence for uo-drawingml** (its record is not edited here): its real-document capture
  renders in Playwright Chromium or the desktop Electron (`write/tools/corpus-lab/run-ours.mjs:29,
  128–133`). Its A12(a) ("pow facets are references only if that run is on Linux with
  `GLIBC_TUNABLES` in the browser launch env") and G2 ("`--no-use-std-math-pow` … for the browser
  run too, if Chromium/Electron accept `--js-flags`") assume the browser can be made a math
  reference; it cannot. Every facet whose path calls sin/cos/tan/atan/atan2/exp/log/pow/hypot (shade/
  tint, preset geometry, effects) must be captured by driving the same esbuild bundle in Node
  24.15 linux-x64 (the drawingml functions are pure; no canvas needed), and the browser run keeps
  only math-free facets. uo-drawingml A12a and G2 must be reworded at its gate.

### Browsers: what "parity with the TS" means

The parity reference is the frozen TS **running in Node 24.15.0 x64 with the recorded platform**
(G2). The same TS already computes different bits in each browser:

- **Chrome/Edge/Electron (V8)**: sin/cos from glibc's `s_sin.c` in clang builds of V8 13.x, and
  LLVM libc for almost all functions since mid-2026 (Chromium 152 measured: 8 of 11 functions
  differ from Node 24). The desktop app (Electron) is in this group.
- **Safari (JSC)**: the system (Apple) libm; `pow` has its own path (`MathCommon.cpp`
  `operationMathPow`): `y = ±0.5` via `sqrt`, small non-negative integer `y` by repeated
  multiplication, otherwise `std::pow` with an fdlibm fallback for denormals/zero results.
  (Read in the WebKit source by the planner; not checked by the skeptic.)
- **Firefox (SpiderMonkey)**: an fdlibm import for most functions (switchable to native
  sin/cos/tan); `ecmaPow` routes int32 `y` to `powi`, which multiplies repeatedly only for small
  `|y|` (n ≤ 4) and has an int64 path, else `std::pow` *(corrected after the skeptic)*.

Implication: the Rust core will give the reference bits everywhere (native, Node, every
browser), so 0.10 is *more* consistent than the TS; a browser user may see a 1-ulp difference
against what the TS showed there, visible only where a result crosses a threshold (a line break,
a rounded coordinate). Parity is not measured against browsers. Under (a) the reference `pow`
is also the one closest to what users run (0.05–0.08 %); a later move of the whole reference to
correctly rounded (where V8 is heading) would be a post-0.10 decision with a full recapture.

### Enforcement *(revised after the skeptic, S11, S12)*

`core/clippy.toml` gets a workspace-wide `disallowed-methods` list (each with a `reason` naming
the `uo_jsnum` replacement). Probed by the skeptic with clippy 1.97.1: it catches method calls,
`f64::sin(x)`, `<f64>::sin(x)`, `.map(f64::sin)`, fn pointers and macro expansions.

- `f64::` and `f32::` `sin`, `cos`, `tan`, `sin_cos`, `asin`, `acos`, `atan`, `atan2`, `sinh`,
  `cosh`, `tanh`, `asinh`, `acosh`, `atanh`, `exp`, `exp2`, `exp_m1`, `ln`, `log`, `log2`,
  `log10`, `ln_1p`, `cbrt`, `hypot`, `powf`, `powi` (LLVM `powi` is repeated multiplication),
  `mul_add` (fused), `round` (half away from zero), `round_ties_even`, `max`/`min` (IEEE maxNum:
  ignore NaN, no ±0 order), `clamp` (panics on NaN bounds; JS idiom differs), `signum`
  (signum(±0) = ±1), `rem_euclid`, `div_euclid`, `abs_sub`, **`to_degrees`, `to_radians`** (Rust
  multiplies by one rounded constant; the TS idiom `x * 180 / Math.PI` differs in 255,004 / 1M),
  **`midpoint`**.
- Strings: `str::trim`, `str::trim_start`, `str::trim_end`, `str::split_whitespace`,
  `char::is_whitespace` (Unicode White_Space: has U+0085, lacks U+FEFF), `str::trim_ascii*`,
  `u8::is_ascii_whitespace`, `char::is_ascii_whitespace` (lack VT).
- **Dependencies**: `cargo deny` (`core/deny.toml`, `[bans]`) denies `libm` and `num-traits` as
  dependencies of the engine crates, so a generic `T: Float` cannot reintroduce std or libm math
  (listing trait paths in clippy works only if the crate is a dependency; denying the crate is
  simpler). A future transitive need goes through `wrappers` with a recorded reason.
- Allowed (exact in IEEE, identical everywhere): `+ − × ÷`, `%` on f64 (fmod is exact; native UCRT
  `fmod` equals JS on the edge cases, S3; the differential covers native and wasm), `sqrt`,
  `floor`, `ceil`, `trunc`, `abs`, `copysign`, `as f32`/`as f64` between floats,
  `is_nan`/`is_finite`, `to_bits`/`from_bits`.
- **Float → int casts** (S12, instead of a blanket deny): a CI step runs `cargo clippy` on the
  engine crates with `-W clippy::cast_possible_truncation -W clippy::cast_sign_loss
  --message-format=json` and fails only on diagnostics whose source type is `f32`/`f64` (the
  int → int hits — 80 in `uo-opc --lib` alone — are ignored). Float → int goes through
  `uo_jsnum::conv` or a named helper. The bug is reachable: docx-core `docx-writer.ts:1427,
  1434` and `pictures.ts:100` (`(prop ?? 0) | 0` on `.doc` property values: ≥ 2^31 wraps negative
  in JS, `as i32` saturates); docx-layout `page-fill.ts:198` `Math.abs((… << 24) | 0)` gives
  2147483648 for int32 MIN in JS, while `i32::abs` overflows — `Math.abs`/negation of an int32
  result is computed in f64.
- CI greps (not lintable): `parse::<f32|f64>`, `f64::from_str` outside `uo-jsnum` (Rust accepts
  `inf`/`nan`, rejects whitespace); `core::f64::consts::` other than the six JS constants (see
  API); `{}`-formatting of floats in engine output is a parity-verifier checklist item.
- Opt-in: a test, bench or example that needs a std function writes
  `#[expect(clippy::disallowed_methods, reason = "…")]` on the item or module. Existing code hit
  by the list: `uo-opc/examples/opc_bench.rs` (`f64::min` on timings, lines 47, 79, 88) gets that
  attribute in the same change. `uo-jsnum` itself needs no exemption.
- **Count test (required)**: an unresolvable path is silently ignored (measured: a nonexistent
  method, or a crate that is not a dependency, gives no diagnostic). `tests/clippy-ban/` (outside
  the workspace members, depending on everything whose paths it bans) calls every listed path
  once; CI asserts the number of `disallowed_methods` diagnostics equals the list length. CI
  already runs `cargo clippy --workspace --all-targets --all-features`; add the same for
  `--target wasm32-unknown-unknown`.

### Parity plan

The reference is not the TS of an app but V8 itself, so the capture is a generator, not a TS
run: `js/capture-jsnum.mjs` (uses `uo-testkit` `js/testkit.mjs`: `Rng`, `Stream`,
`CaptureWriter`), run in WSL as `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2 node
core/crates/uo-jsnum/js/capture-jsnum.mjs` under G1 (a) (under (b): `node
--no-use-std-math-pow …` on any x64 machine).

- **Inputs, identical on both sides** from `Rng` (xorshift32): `bits` = `f64::from_bits(hi << 32
  | lo)` from two draws (every class: NaN payloads, subnormals, huge); `u53 = ((a >>> 5) ·
  67108864 + (b >>> 6)) / 2^53` and `lo + (hi − lo) · u53` for ranges (same IEEE operations in JS
  and Rust). One seed per function and distribution, recorded.
- **Distributions** (10^7 samples each): sin/cos/tan: [−8π, 8π], [−2^20, 2^20], bits; asin/acos:
  [−1, 1], bits; atan: [−200, 200], bits; atan2: pairs [−2e4, 2e4]², bits²; exp: [−745, 710],
  bits; expm1: [−40, 710], bits; log/log2/log10: (0, 1000], |bits|; log1p: (−1, 1000], bits;
  sinh/cosh: [−720, 720]; tanh: [−25, 25]; atanh: (−1, 1); cbrt: bits; pow: x ∈ [0, 2) × y ∈
  {2.4, 1/2.4, 2.2, 1/2.2, 3, −1, 10, 0.5, 2}, x ∈ [0, 100) × y ∈ [−5, 5), x ∈ (0, 1e3] × integer
  y ∈ [−20, 20], `pow(10, n)` for n ∈ [−350, 350], bits²; hypot: pairs [−1e4, 1e4]², triples,
  bits², 4–6 arguments; round/sign/trunc/floor/ceil: bits, [−10, 10], k + 0.5 for k ∈ [−2^53,
  2^53]; max/min: pairs over specials ∪ bits; conv::*: bits, [−2^40, 2^40]; `js_index`: bits,
  [−2, len + 2] with fractions; `%`: bits², [−1e6, 1e6]² (10^6).
- **Special values** (all outputs written in full, hex, not hashed, so a failure is readable):
  ±0, ±Inf, NaN, ±MIN_VALUE, ±MAX_VALUE, smallest normal, ±1, ±0.5, ±2, ±π/4·k, 1e22, 2^1023,
  every high-word threshold in each ported function (`ix < 0x…` tests) with low word 0, 1 and
  0xFFFFFFFF and both signs; binary functions over all pairs of that list; the ES `pow` special
  cases table.
- **Strings** (A: `to_number`, `parse_int`, `text`; B: `parse_float`, formatters): a handwritten
  corpus (each JS space char and U+0085/U+180E as negatives, `0x`, `0X1a`, `-0x10`, `0b101`,
  `0o17`, `Infinity`, `-Infinity`, `infinity`, `1e1000`, `1e-400`, `.5`, `5.`, `.`, `+.5e-3`,
  `1_000`, 800-digit strings on halfway points, hex above 2^53 on ties) plus the skeptic's cases
  (S9: `Number('-0x10')` NaN, `Number('+0x10')` NaN, `Number('1e')` NaN, `Number('5.')` 5,
  `Number('.')` NaN, `parseFloat('1_0')` 1, `parseFloat('Infinityx')` ∞, `parseFloat('+-1')` NaN,
  `parseInt('-0x10')` −16, `parseInt('0x10', 10)` 0, `parseInt('10', 4294967312)` 16,
  `parseInt('9007199254740993')` 9007199254740992; S7: `(-0).toFixed(2)` `"0.00"`,
  `(-1e-10).toFixed(2)` `"-0.00"`, `(-0).toPrecision(3)` `"0.00"`, `(0.5).toFixed(0)` `"1"`,
  `(-0.5).toFixed(0)` `"-1"`, `(25).toExponential(0)` `"3e+1"`), plus 10^6 random strings from a
  token alphabet per function; `parse_int` radices {0, 2, 7, 8, 10, 16, 36, 1, 37, 4294967312};
  `utf16_len`/`utf16_slice` against `s.length`/`s.slice` on strings with astral characters and
  every cut position. Formatting (B): `to_js_string` on 10^7 bits and 10^7 `k/10^d`; `to_fixed` d
  ∈ [0, 100], `to_precision` p ∈ [1, 100], `to_exponential` d ∈ [0, 100], each on 10^6 inputs
  from bits, [−1e21, 1e21], dyadic ties `k/2^j` and `k/10^d`; `to_radix_string` radix ∈ [2, 36]
  on 10^6 integers in (−2^53, 2^53); out-of-range arguments map to `RangeError` = JS throw.
- **Comparison**: exact bits; NaN outputs canonicalized to `0x7ff8000000000000` on both sides
  before hashing. **Rule for every engine crate** *(revised after the skeptic, S5)*: NaN payloads
  are observable in the TS (`DataView.setFloat64(Math.log(-1))` writes `0x7ff8000000000001`,
  `Math.sqrt(-1)` writes `0xfff8000000000000`), so Rust canonicalizes NaN
  (`conv::canonicalize_nan`) before any byte serialization or bit inspection, captures are
  compared canonicalized, and no output may depend on payload bits (parity-verifier checklist).
  Strings through `Stream::str`. Each record keeps a digest at 10^5 and at 10^7 samples.
- **Tiers**: the generator runs every distribution twice — default and `--no-opt --no-maglev` —
  and records `@tiers-agree` (a JIT reduction that differs from the builtin would show here).
- **Replay**: `tests/differential.rs` checks the 10^5 digests in plain `cargo test`, and the 10^7
  digests in `#[ignore]` tests run by CI with `cargo test -p uo-jsnum --release --
  --include-ignored`. On a mismatch, `UO_JSNUM_DUMP=<fn>` makes the Node script write the raw
  outputs to `target/jsnum-dump/` (not committed) and the test prints the first 20 differing
  inputs.
- **wasm = native = capture**: `uo-wasm` feature `jsnum-selftest` exports `jsnum_digest(fn_id,
  seed, n)`; `core/crates/uo-wasm/tests/jsnum.test.mjs` compares it with the capture digests in
  Node and in WebKit (`tests/browser/run.mjs`).
- **Capture metadata** (`uo-testkit`, for every module from now on) *(revised after the
  skeptic, S3)*: `CaptureWriter` writes `@platform node=v24.15.0 v8=13.6.233.17-node.48 os=…
  arch=… cpu=<model> features=<cpuid mask> glibc=<version or none> tunables=<GLIBC_TUNABLES>
  flags=<execArgv>` automatically (`platform()` in `testkit.mjs`). The Rust `Capture` gets
  `require_platform(&PlatformSpec)`; every replay of a capture whose path calls a `uo-jsnum` math
  function calls it. It always checks the V8 prefix `13.6.233.17` and `arch=x64`; under G1 (a)
  also `os=linux`, `glibc=2.39` and the hwcaps mask `-FMA,-AVX2`; under (b) the flag. Captures
  that predate it (uo-opc) contain no math and are exempt.

### Memory plan

- **M1**: every string-returning function under `uo_alloc::assert_no_leak` (outputs dropped);
  every numeric function (and `utf16_slice` without a cut pair) under `measure` with **0 bytes
  allocated** (asserted, not just recorded).
- **M2**: soak of 1,000 iterations of a mixed batch (10^4 calls of every function, strings
  dropped), sampled every 50; tolerance **0 B** (no caches).
- **M3**: allocated bytes per string function = the output's capacity; peak ≤ 2× output length;
  the record keeps `to_fixed(1e20, 100)` (≤ 122 B), `to_radix_string(-(2^53 − 1), 2)` (≤ 54 B),
  `Js` (0 B). Also the crate's wasm size contribution per increment (twiggy over `uo-wasm
  --profile wasm`).
- **M4**: no handles cross a boundary (the wasm self-test export takes and returns plain
  numbers).
- **M5**: no caches; the M2 tolerance proves it.
- **M6**: no `unsafe` (forbidden), so no Miri job.

### Security plan

- Untrusted inputs: every string from a document attribute (`to_number`, `parse_float`,
  `parse_int`, `trim`, `split_js_space`, `utf16_slice` with document-derived indices) and every
  float from a document reaching the math and format functions or `js_index`.
- Limits: parsing is linear in the input (a 10 MB digit string parses in bounded time; test);
  no allocation proportional to input in the parsers; formatting output bounded (≤ 54 B radix,
  ≤ 122 B fixed); `RangeError` instead of panic for argument ranges; `utf16_slice` clamps every
  index (i64 extremes tested); tests run with overflow checks so a missed `wrapping_*` in the
  fdlibm or pow port panics in CI.
- Fuzz targets in `core/fuzz`: `jsnum_parse` (A: bytes → lossy UTF-8 → `to_number`, `parse_int`
  with a fuzzed radix, `utf16_slice` with fuzzed indices; property: `to_number` of a decimal
  string printed by the test harness's exact formatter returns the same bits; B adds
  `parse_float` and `to_number(Js(x))` = bits of `x`), `jsnum_format` (B: f64 bits × digits →
  the four formatters; no panic, `to_number(to_fixed(x, 100))` within the decimal rounding of
  `x`), `jsnum_math` (A: f64 bits pairs → every math function and `js_index`; properties: no
  panic in debug, NaN in → NaN out where the spec says, |sin|,|cos| ≤ 1).

### Risks for the port (checklist for the verifiers)

1. Operation order: no algebraic simplification, no `mul_add`, keep C parentheses; Rust never
   contracts to FMA; V8 does not either **on x64** (arm64 Node builds may, S3).
2. C integer conversions: `(int32_t)(bits >> 32)` = `as u32 as i32`; `static_cast<int>(x)` only
   where the source guards the range; unsigned arithmetic `wrapping_*`.
3. Constants from hex bits; test against the decimal literal.
4. `hypot`: Infinity before NaN; the 3-argument path is not the Kahan loop.
5. `round` of −0.5…−0 → −0; `0.49999999999999994` → 0; `max/min` with NaN and ±0; `sign(±0)`.
6. `Uint8ClampedArray` round half to even; `Uint8Array` wraps (`as u8` saturates); `Float32`
   store = `as f32`; float-indexed typed-array access through `js_index`.
7. `parse_int`: `-0` stays −0 (`parseInt("-0")`), `0x` only when radix ∈ {0, 16} and a sign may
   precede it, junk stops the scan, radix is ToInt32'd; `to_number`: `""`/whitespace → 0, no sign
   before `0x`, no `_`, `Infinity` is case-sensitive.
8. NaN: canonicalized before bytes and comparisons; no code may depend on payload bits.
9. `Math.abs`/negation of an int32 result (`x | 0`, shifts) is f64 arithmetic, never `i32::abs`
   (S12).
10. pow under (a): the non-FMA branch everywhere (no `#[cfg(target_feature = "fma")]`), the V8
    wrapper cases before the core, tables copied from the pinned optimized-routines commit.

### Models per part *(revised after the skeptic, S15; replaces "Sonnet readiness")*

Closed for every part: API and signatures above, file layout, the source of every function with
line numbers at a fixed tag, the input distributions and seeds rule, the comparison rule and the
ban list. The fdlibm translation has an exact oracle that localizes a failure to one function and
input (S1).

| Part | Increment | Model | Why |
|---|---|---|---|
| fdlibm: `rem_pio2`/`kernel_rem_pio2`, kernels, the 19 functions | A | Sonnet | line-by-line translation (~2,300 lines of C → ~1,800 of Rust) against an exact oracle |
| `pow` under G1 (a): optimized-routines core + V8 wrapper | A | **Opus** | branch selection (non-FMA), tables, wrapper semantics, licence provenance of the copy |
| `pow` under G1 (b): `legacy::pow` | A | Sonnet | part of the same `ieee754.cc` translation |
| `hypot`/`hypot3`/`hypot_n` (Torque) | A | Sonnet | short, exact source |
| spec functions (`round`, `sign`, `max`/`min`/`_n`, `imul`, `clz32`, `fround`, wrappers), `conv` (with `js_index`, `canonicalize_nan`) | A | Sonnet | exactly defined by ECMA-262 |
| `text` (whitespace, `trim`, `split_js_space`, `utf16_len`/`utf16_slice`) | A | Sonnet | small, exhaustively testable over code points |
| `to_number` (grammar validation), `parse_int` (`HandleGenericCase`, power-of-two paths) | A | Sonnet | V8 source at the tag + a strong corpus; moderate spec work |
| `parse_float` | B | Sonnet | same grammar as `to_number`, prefix form |
| `Js`/`to_js_string` (ryu-js glue) | B | Sonnet | dependency call |
| `to_fixed`/`to_precision`/`to_exponential` | B | **Opus** as designed (stack-bignum design: digit generation, exponent choice at power-of-ten boundaries, tie rule); **Sonnet** only if G13 turns it into a line-by-line port of V8's `DoubleToFixedCString`/`DoubleToPrecisionCString`/`DoubleToExponentialCString` + `fixed-dtoa`/`bignum-dtoa` | judgment vs translation |
| `to_radix_string` (integers only) | B | Sonnet | trivial after S8 |

## Questions for the maintainer (gate 1) — consolidated *(planner + skeptic)*

| # | Question | Final recommendation |
|---|---|---|
| G1 | `pow`: (a) Linux capture (WSL, glibc 2.39, non-FMA forced) + port of ARM optimized-routines `pow` with V8's std-branch special cases; (b) `--no-use-std-math-pow` + V8 `legacy::pow`; (c) correctly rounded; (d) Windows UCRT | **(a)** (S2: 0.05–0.08 % from what users run, vs 6.3 % for (b); (b) is not correctly rounded in 8.2 % and breaks `10^-4`/`10^-5`). **(b)** only if Linux captures are rejected, with the cost in `CHANGELOG.md` and no later correctly-rounded switch |
| G2 | Pin the 0.10 parity reference to Node `v24.15.0` (V8 13.6.233.17) **x64**, enforced by `@platform` + `require_platform`; under (a) also `os=linux`, `glibc=2.39`, hwcaps `-FMA,-AVX2`; under (b) the flag. Every capture whose path calls a `uo-jsnum` math function runs in that Node — never in Chromium/Electron (affects uo-drawingml A12a/G2). A Node/V8 upgrade means recapturing every module | **Yes** |
| G3 | `uo-jsnum` contains BSD-3-Clause code (V8, with the Sun fdlibm notice) and, under (a), optimized-routines code (MIT OR Apache-2.0 WITH LLVM-exception): crate `license = "MIT AND BSD-3-Clause"` (optimized-routines is dual-licensed; we take it under MIT), `LICENSE-V8`/`LICENSE-optimized-routines` in the crate, entries in the npm/NuGet `THIRD-PARTY-NOTICES.md` | **Yes** |
| G4 | `ryu-js =1.0.3` (Apache-2.0 OR BSL-1.0) for Number::toString in increment B | **Yes** (S6: byte-identical on 3M values) |
| G5 | Float → int casts: CI filter over the JSON output of `cast_possible_truncation`/`cast_sign_loss`, failing only on `f32`/`f64` sources; float → int only through `uo_jsnum::conv` or named helpers | **Yes** (replaces the planner's "deny in uo-drawingml first") |
| G6 | Workspace ban list (math incl. `to_degrees`/`to_radians`/`midpoint`, rounding, min/max, clamp, signum, whitespace methods) with `#[expect(…, reason)]` for tests/benches; `cargo deny` bans of `libm`/`num-traits` in engine crates; the count test; CI greps for `parse::<f64>` and non-JS `f64::consts` | **Yes** |
| G7 | `uo-opc` drops its `is_js_space`/`js_space_len` and depends on `uo-jsnum` after increment A's gate 2 | **Yes** |
| G8 | Not `no_std` (core lacks sqrt/floor/ceil/trunc in Rust 1.97) | **Yes** |
| G9 | Accept that the Rust core gives the reference bits in every browser (ulp-level differences from what the TS computes in Chrome ≥ ~149, Safari, Firefox); browser results are measured for the record, not as a gate | **Yes** |
| G10 | Install Node 24.15 linux-x64 in WSL Ubuntu-24.04 (captures under (a); the platform table) | **Yes** (required by G1 (a)) |
| G11 | Two increments with their own gate 2: **A** now (unblocks uo-drawingml), **B** (`format`, `parse_float`) before uo-pdf/uo-docx/uo-charts | **Yes** |
| G12 | Add `text::utf16_len`/`utf16_slice` (cut halves → U+FFFD; exact for parsing/whitespace); lowercasing stays in uo-fonts (its private `js` module); uo-fonts does not wait for uo-jsnum; one name `split_js_space` (uo-drawingml R9 uses it) | **Yes** |
| G13 | Models per part (table above): increment A on Sonnet except `pow` on Opus under (a); increment B's exact formatter on Opus, or on Sonnet if rewritten as a line-by-line port of V8's `DoubleTo{Fixed,Precision,Exponential}CString` + `fixed-dtoa`/`bignum-dtoa` | **Yes**; prefer the V8 line-by-line port for B (exact oracle, Sonnet) |
| G14 | `to_radix_string` only for integral \|x\| < 2^53 (every engine caller formats integers) | **Yes** |
| G15 | Workspace rules: canonicalize NaN before any byte serialization or bit inspection; `max_n`/`min_n`/`hypot_n` never fail (the TS throws above ~120k–200k spread arguments) — accepted divergence, recorded as an error outcome in captures | **Yes** |
| G16 | uo-drawingml's requests (its G15): `conv::js_index` (added to increment A); `to_radians`/`to_degrees` banned (added); only the six JS constants of `f64::consts` (CI grep) | **Yes** |

## Analysis skeptic (verified in the code)

port-skeptic-analysis (Opus), 2026-11-01. Measurements on this machine (Windows 11 x64, FMA CPU,
Node v24.15.0 / V8 13.6.233.17-node.48, Rust 1.97.1, MinGW g++ 16.2, WSL Ubuntu-24.04 glibc
2.39); scratch in `%TEMP%/skjs` (not committed): `c/harness.cc` compiles V8 13.6
`src/base/ieee754.cc` standalone (stub `macros.h`/`overflowing-math.h`, `base::Divide` = `/`),
`gen.mjs`/`cmp.mjs` (1M samples per function, the plan's ranges + random bits), `gpow.c` (glibc
`pow` in WSL), a 60-digit `decimal` correctly-rounded `pow` reference (Python), `ryu/` (ryu-js
1.0.3), `fmt.mjs` (BigInt exact-spec reference of `toFixed`/`toPrecision`/`toExponential`),
`ban/` (clippy `disallowed-methods` probe).

| # | Finding | Blocking | Proposed decision |
|---|---|---|---|
| S1 | **Claim (1) confirmed by measurement.** V8 13.6 `ieee754.cc` compiled with `g++ -O2 -ffp-contract=off` equals Node 24.15 `Math.*` bit for bit on 1M inputs each for all 20 functions (sin, cos, tan, asin, acos, atan, atan2, exp, expm1, log, log1p, log2, log10, sinh, cosh, tanh, atanh, cbrt, and the unused asinh/acosh); `legacy::pow` equals `node --no-use-std-math-pow` on 1M pairs, also with `--no-opt --no-maglev`. Only NaN payloads differ (asin/acos/log/log1p/log2/pow). Static evidence agrees: the glibc `s_sin.c` path exists only in GN (`v8.gni:169` `v8_use_libm_trig_functions = is_clang`, `BUILD.gn:1402`, source_set `libm` 6759–6777); Node's `tools/v8_gypfiles/v8.gyp`/`features.gypi` neither define `V8_USE_LIBM_TRIG_FUNCTIONS` nor compile `third_party/glibc`. | no | Recorded. The port is a translation against an exact oracle. |
| S2 | **pow: option (b) makes the Rust core reproduce the least accurate `pow` of every platform users run.** Same 1M pairs (4 distributions × 250k): default Node (Windows, UCRT) vs the flag differ in 62,994 (sRGB/gamma exponents 15,906; x∈[0,100)×y∈[−5,5) 24,670; integer y∈[−20,20] 22,339); glibc 2.39 (FMA ifunc) vs UCRT 542; glibc non-FMA (`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2`) vs UCRT 769; glibc FMA vs non-FMA 549. Against a correctly-rounded reference (37,428 finite non-trivial pairs): UCRT not CR in 18, glibc 36 (FMA) / 41 (non-FMA), legacy fdlibm **3,070 (8.2%)**. `Math.pow(10, n) !== Number('1e' + n)`: default only n = 23; flag n ∈ {−39, −32, −29, −24, −23, −21, −20, −17, **−5, −4**, 26, 29, 34, 36, 39} — with the flag `String(Math.pow(10, -4))` is `"0.00009999999999999999"` and `7.5 / Math.pow(10, -5)` is `750000.0000000001`. TS users of `Math.pow(10, e)` with negative `e`: charts `numfmt.ts:292, 824–827`, `scale.ts:17`, docx-core `table-formula.ts:183, 242`, calc `text-format.ts:315`, sheets-processing `format.ts:150–157, 420–424`. drawingml: `SRGB_LIN` 33/256 entries change, blackbody (adjust.ts:52–53) 65/668, the rounded `LIN_SRGB` 0/4096. So: what users get (default Node, Chrome/Electron on Windows = UCRT; Linux servers/Chrome = glibc) is served better by (a), ≈0.05–0.08 % different, than by (b), 6.3 % different; (b) wins only on capture convenience (one Windows machine, same BSD file, Sonnet). | yes (G1 must be decided on these numbers, not on "1 ulp, ~10 %") | Skeptic recommends **(a)**: Node 24.15 in WSL (G10 becomes required), captures with the non-FMA glibc path forced, port ARM optimized-routines `pow` (non-FMA branch) + V8's `math::pow` std-branch special cases, on Opus. If the maintainer prefers (b), write the `10^-4`/`10^-5` cost in `CHANGELOG.md` and do not plan a later CR switch (it would re-capture every module). |
| S3 | **The reference depends on the architecture, not only on Node/V8 and flags.** Node's gyp sets `-ffp-contract=off` only for s390x and ppc64 (`toolchain.gypi:307, 328` at v24.15.0); arm64 builds compile `ieee754.cc` with the compiler default (gcc: `fast`; clang: `on`). Contraction simulated on the same source (`g++ -O2 -mfma -ffp-contract=fast`, x64) vs Node x64: sin 4,313, cos 4,284, tan 8,136, atan 24, atan2 496, exp 232, log 1, sinh 483, legacy pow 309 per 1M; log10/cbrt 0. Risk 1 "V8 does not either" holds for x64 only. (Not measured on a real arm64 Node.) The Rust side is safe: rustc never contracts, `mul_add` is banned; native `%` (UCRT `fmod`) equals JS on finite % ±∞, ±0 % y, subnormal cases (10 edge cases measured). | yes (cheap) | `require_platform` checks `arch=x64` as well as the V8 prefix and flags (G2); the macOS arm64 row of the platform table is informative and expected to differ. |
| S4 | **Captures taken in a browser cannot be math references, with any flag.** The real-document capture of uo-drawingml runs in Playwright Chromium or the desktop Electron (`write/tools/corpus-lab/run-ours.mjs:29, 128–133`); Chromium 152 differs from Node 24 on 8 of 11 functions (the planner's measurement), its `legacy::pow` is LLVM libc since `716d3e3363` (so `--js-flags=--no-use-std-math-pow` does not give fdlibm pow), and a clang-built V8 13.6 (Chromium 136) still takes glibc `s_sin.c` for sin/cos. uo-drawingml A12/G2 assume the flag is enough for the browser run. | yes (G2 wording) | G2: every capture whose path calls a `uo-jsnum` math function runs in Node 24.15.0 x64 with the recorded flags/env; browser runs are references only for math-free outputs. Tell uo-drawingml (A12a). |
| S5 | **NaN payloads are observable in the TS.** Node writes `Math.log(-1)` → `0x7ff8000000000001`, `Math.sqrt(-1)` → `0xfff8000000000000`, `0/0` → `0x7ff8000000000000` through `DataView.setFloat64`; the TS has such stores and bit inspection (calc `libm.ts:84`, DataView float setters in docx-layout/slides-core). Comparison after canonicalization is right for this crate; downstream it is not automatic. | no | Rule for the engine crates (parity-verifier checklist): canonicalize NaN before any byte serialization or bit inspection; no output may depend on payload bits. |
| S6 | **ryu-js = V8 Number::toString, measured.** 3,000,000 values (random bits, `k/10^d`, wide decimals, 22 specials: ±0, NaN, ±∞, ±MIN_VALUE, MAX_VALUE, smallest normal, 1e21, 1e20, 1e-7, 1e-6, 2^53+1, 1e23, …): `ryu_js::Buffer::format` byte-identical to `String(x)`. | no | G4 confirmed. |
| S7 | **V8's `toFixed`/`toPrecision`/`toExponential` are exactly the spec rule** (exact binary value, ties to the larger magnitude, sign separate): 960,064 calls (digits 0, 1, 2, 3, 5, 10, 20, 100; random bits, dyadic ties `k/2^j`, decimal fractions) agree with a BigInt reference. Edge cases to add to the special list: `(-0).toFixed(2)` = `"0.00"`, `(-1e-10).toFixed(2)` = `"-0.00"`, `(-0).toPrecision(3)` = `"0.00"`, `(0.5).toFixed(0)` = `"1"`, `(-0.5).toFixed(0)` = `"-1"`, `(25).toExponential(0)` = `"3e+1"`. Uses in the engine: `toPrecision` only with 2, 12, 15; `toExponential` only with 14; `toFixed` 0–6 and variable `d`. | no | Keep the exact formatter; add the cases. |
| S8 | **`to_radix_string` has no user for non-integers.** Every engine caller formats an integer: `(h >>> 0).toString(36)` (slides-core `emfplus.ts:126`), `toString(16)` of bytes/u32/gids, calc `formatBase` (`engineering.ts:66–67`, integers inside the base range, radix 2/8/16). The `DoubleToRadixStringView` fraction loop (delta rule, 2,200-byte buffer) is the riskiest Sonnet item and unused. | no | `to_radix_string` for integral \|x\| < 2^53 (exact digits, `RangeError`-like `Err` otherwise); fraction support only when a caller appears. |
| S9 | **Parser rules verified.** Enumerating every code point: the sets of `Number()`, `/\s/` and `trim` are identical and equal the plan's set (U+0009–000D, 0020, 00A0, 1680, 2000–200A, 2028, 2029, 202F, 205F, 3000, FEFF; not 0085, 180E). Outputs that the corpus must hold (all as the plan's risk 7 says): `Number('-0x10')` NaN, `Number('+0x10')` NaN, `Number('1e')` NaN, `Number('5.')` 5, `Number('.')` NaN, `parseFloat('1_0')` 1, `parseFloat('Infinityx')` ∞, `parseFloat('+-1')` NaN, `parseInt('-0x10')` −16 (sign before `0x` allowed in `parseInt`), `parseInt('0x10', 10)` 0, `parseInt('10', 4294967312)` 16 (ToInt32 of the radix), `parseInt('9007199254740993')` 9007199254740992. | no | Add the listed cases to the string corpus. |
| S10 | **API gaps against the dependent plans.** uo-drawingml R9 expects `jsnum::utf16_slice` and `split_js_whitespace`; uo-fonts S4/S15 needs code-unit `length`/`slice` of untrusted attributes and lowercasing. The plan has neither and `parse_int(&str)`. Every numeric parser and the whitespace tests treat any non-ASCII, non-space character as junk, so a cut surrogate half can be U+FFFD without changing a result. The "planned crates" line for uo-fonts is wrong (uo-fonts uses `ilog2`/shifts, no `log2`, `**` or `round`). | no (before stage 3) | Add `text::utf16_len(&str) -> usize` and `text::utf16_slice(&str, start: i64, end: i64) -> Cow<str>` (JS `slice` clamping; cut halves → U+FFFD; documented as exact for parsing and whitespace, not for re-emitting text). Name `split_js_space` once and tell uo-drawingml. Lowercasing is string semantics, not numerics: stays in uo-fonts (its S15) — G12. Fix the uo-fonts line. |
| S11 | **Clippy ban probed (clippy 1.97.1).** `disallowed-methods` catches `x.sin()`, `f64::sin(x)`, `<f64>::sin(x)`, `.map(f64::sin)`, a `fn` pointer, a macro expansion, and trait methods when the trait path is listed (`num_traits::float::Float::sin` caught both `T: Float` generic code and `Float::sin(x)`). An unresolvable path is silently ignored (`f64::nonexistent_method`, `libm::sin` when `libm` is not a dependency: no diagnostic), so the count test is needed and its crate must depend on what it bans. Missing from the list: `f64::to_degrees`/`to_radians` (Rust is `x * (180/π)` with one rounded constant, core `f64.rs:994`; the TS idiom `x * 180 / Math.PI` differs in 255,004 / 1M samples) and `f64::midpoint`. `core::f64::math` and intrinsics are unstable, unreachable on stable. | no | Add `to_degrees`, `to_radians`, `midpoint` (f64 and f32). Instead of listing `num_traits`/`libm` paths, deny those crates as dependencies of engine crates (`cargo deny` `bans`), so a generic `Float` cannot reintroduce std math. |
| S12 | **float → int `as`: the lint is too noisy as proposed, and the bug is reachable.** `cast_possible_truncation` flags `f64 as i32/u8` (verified) but `uo-opc --lib` alone gets 80 warnings, all int → int, 0 float. Reachable divergences: docx-core `docx-writer.ts:1427, 1434` and `pictures.ts:100` (`(prop ?? 0) \| 0` on .doc property values: ≥ 2^31 wraps negative in JS, `as i32` saturates); docx-layout `page-fill.ts:198` `Math.abs((… << 24) \| 0)` — JS gives 2147483648 for int32 MIN, Rust `i32::abs` overflows. | no | G5: a CI step runs clippy with `-W clippy::cast_possible_truncation -W clippy::cast_sign_loss --message-format=json` on the engine crates and fails only on diagnostics whose source type is `f32`/`f64` (zero noise); float → int only through `uo_jsnum::conv` or a named helper. Add "`Math.abs`/negation of an int32 result is f64" to the verifier checklist. |
| S13 | **Spread arity.** `Math.max(...a)` throws `RangeError` in Node 24 between 120k (ok) and 200k elements (measured); charts `render.ts:587–733` spreads document-sized arrays (legend widths). `max_n`/`min_n`/`hypot_n` never throw. | no | Accepted divergence (Rust is total); capture generators record the TS exception as an error outcome, never as a value. |
| S14 | **Scope of the first gate is wider than its consumers.** uo-fonts needs nothing from uo-jsnum if its S15 is accepted. uo-drawingml needs: math {sin, cos, tan, atan, atan2, exp, log, pow, hypot/hypot3/hypot_n, round, sign, max/min (+ `_n`), floor/ceil/trunc/abs/sqrt}, conv {to_uint8, to_uint8_clamp, to_int32, fround}, text {is_js_space, trim, split_js_space, utf16_*}, parse {to_number, parse_int}. No formatter, no `parse_float`. The other fdlibm functions are cheap once `rem_pio2`, the kernels and `k_log1p` exist and share the harness. | no | Two increments in this record: **A** (gate now; unblocks uo-drawingml): all of `fdlibm`, hypot, pow, the spec functions, `conv`, `text`, `to_number`, `parse_int`. **B** (own gate before uo-pdf/uo-docx/uo-charts): `format` (`Js` via ryu-js, `to_fixed`/`to_precision`/`to_exponential`, integer radix) and `parse_float`. G11. |
| S15 | **Model per part (resolves planner Sonnet vs uo-drawingml Opus).** Mechanical, with an exact oracle that localizes a failure to one function and input (S1): the fdlibm translation including `__kernel_rem_pio2` and `legacy::pow`, the Torque hypot, `round`/`sign`/`max`/`min`/`imul`/`clz32`/`fround`/`conv`, `text`, and `parse_int`'s `HandleGenericCase`/power-of-two paths (V8 source at the tag). Judgment: the exact decimal formatter (a stack-bignum design, not a translation: digit generation, exponent choice at power-of-ten boundaries, the tie rule) and, under (a), the optimized-routines `pow` (non-FMA branch selection, tables, the V8 std-branch wrapper). `to_number`'s grammar validation is spec work of moderate size with a strong corpus. | no | Increment A on **Sonnet** (with pow on **Opus** if G1 = (a)); increment B's formatter on **Opus**, unless it is changed to a line-by-line port of V8's `DoubleToFixedCString`/`DoubleToPrecisionCString`/`DoubleToExponentialCString` + `fixed-dtoa`/`bignum-dtoa`, which Sonnet can do. G13. |

Discarded (not verifiable here): the Safari/JSC and Firefox claims of "Browsers" (only the
SpiderMonkey source was checked: `jsmath.cpp` `powi` covers integer `y` with `n ≤ 4` repeated
multiplication and an int64 path, else `std::pow` — not "repeated multiplication for every int32
`y`"); the wasm size estimate; macOS/arm64 Node behaviour (S3 is a simulation).

## Verification (gate 2 — pending)

### Parity

### Memory (measured)

| Check | Result | Numbers |
|---|---|---|
| M1 no leak per operation | | |
| M2 soak (native) | | |
| M2 soak (wasm) | | |
| M3 budgets | | |
| M4 boundaries | n/a | |
| M5 cache caps | n/a | |
| M6 Miri / LeakSanitizer | n/a (no unsafe) | |

### Security

## Evolution

| Change | Keeps results? | Before | After | Decision |
|---|---|---|---|---|

## Final review and benchmarks (gate 3 — pending)

## Process metrics

| Stage | Agent / model | Wall time | Rounds | Rework |
|---|---|---|---|---|
| 1 plan | port-planner / Opus | | — | — |
