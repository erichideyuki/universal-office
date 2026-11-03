# uo-jsnum — JavaScript numerics

Record of the rust-port workflow. Status: **gate 1 approved 2026-11-01** (G1–G16 as the final recommendations: pow option (a), two increments); stage 3 and 4 of increment A done, gate 2 approved 2026-11-03; stage 3 of increment B done ("Stage 3 — port (increment B)" at the end), stage 4 of B pending.

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

## Questions for the maintainer (gate 1)

**Approved 2026-11-01.** Maintainer, verbatim: "Eu aprovo tudo" — every final recommendation accepted. pow = option (a). — consolidated *(planner + skeptic)*

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

## Stage 3 — pow (option a)

Port of `math::pow` under G1 (a), Opus, 2026-11-01/02. Files: `src/math/pow.rs` (V8 wrapper +
core), `src/math/pow/log_data.rs`, `src/math/pow/exp_data.rs` (tables), `LICENSE-optimized-routines`,
`js/capture-pow.mjs`, `tests/captures/pow.capture`, `tests/pow.rs`. The pow capture is its own file
(not `jsnum.capture`), so it can be retaken without touching the other functions.

### Reference platform (G2, G10)

- Node **v24.15.0 linux-x64** installed in WSL Ubuntu-24.04 (user `hideyuki`) at
  `~/opt/node-v24.15.0-linux-x64` from the official tarball
  `https://nodejs.org/dist/v24.15.0/node-v24.15.0-linux-x64.tar.xz`; SHA-256
  `472655581fb851559730c48763e0c9d3bc25975c59d518003fc0849d3e4ba0f6`, equal to its line in the
  official `SHASUMS256.txt` (`sha256sum -c`: OK). The GPG signature of `SHASUMS256.txt` was not
  checked. No sudo, nothing added to the system PATH.
- Identity: `process.version` v24.15.0, `process.versions.v8` 13.6.233.17-node.48, arch x64,
  `glibcVersionRuntime` 2.39 (Ubuntu GLIBC 2.39-0ubuntu8.7, the system `libm.so.6`). CPU Intel Core
  i5-14600KF (`fma`, `avx2`; no `fma4`).
- The capture line (written by `uo-testkit` `platform()`): `@platform node=v24.15.0
  v8=13.6.233.17-node.48 os=linux arch=x64 cpu=Intel(R)_Core(TM)_i5-14600KF features=fma+avx2
  glibc=2.39 tunables=glibc.cpu.hwcaps=-FMA,-AVX2 flags=none`. `tests/pow.rs` checks it with
  `Capture::require_platform` (V8 13.6.233.17, x64, linux, glibc 2.39, the hwcaps mask, no flags)
  plus node v24.15.0. The script refuses to write the committed file on any other platform.
- Run (WSL, repository root): `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2 node
  core/crates/uo-jsnum/js/capture-pow.mjs` (~10 s for the default tier, ~60 s in total with the
  `--no-opt --no-maglev` tier).

### Mask verification

glibc 2.39 x86_64 `pow` is an ifunc (`sysdeps/x86_64/fpu/multiarch/e_pow.c` with
`ifunc-fma4.h`): `__ieee754_pow_fma` (built `-mfma -mavx2`) when FMA and AVX2 are usable, else the
`fma4` build when FMA4 is, else `__ieee754_pow_sse2` (the generic `dbl-64/e_pow.c` without
`__FP_FAST_FMA`). The CPU has no FMA4, so `-FMA,-AVX2` selects `sse2`. Measured on the same
8 × 10^7 inputs: masked vs unmasked Node differ in **39,018 / 80,000,000 (0.049 %)**, all by 1 ulp
(the skeptic's S2: 549 / 1M), so the mask changes the path; a second masked run is identical to the
committed capture; and the Rust port of the non-FMA branch equals the masked capture bit for bit
while it differs from the unmasked run, which identifies the masked path as the non-FMA algorithm.

### Source pinned

- Core: ARM optimized-routines (https://github.com/ARM-software/optimized-routines), commit
  **`c0137013d868422fce51de5e5a2af567ff784b7e`** (2024-01-11, "Update copyright years"; glibc 2.39
  was released 2024-01-31), files `math/pow.c`, `math/pow_log_data.c`, `math/exp_data.c`,
  `math/math_config.h`, `math/math_err.c`, taken under the MIT option of "MIT OR Apache-2.0 WITH
  LLVM-exception" (`LICENSE-optimized-routines`, G3). Configuration: `HAVE_FAST_FMA` 0,
  `TOINT_INTRINSICS` 0, `EXP_USE_TOINT_NARROW` 0, `EXP_TABLE_BITS` 7, `EXP_POLY_ORDER` 5 (not
  wide), `POW_LOG_TABLE_BITS` 7, `POW_LOG_POLY_ORDER` 8, `WANT_ROUNDING` 1.
- Diff against glibc 2.39 (`sysdeps/ieee754/dbl-64/e_pow.c`, `e_pow_log_data.c`, `e_exp_data.c`,
  read for the comparison only; nothing copied): the statements of `log_inline`, `specialcase`,
  `exp_inline`, `checkint`, `zeroinfnan` and `pow` are the same under that configuration. The only
  differences are macro names (`__FP_FAST_FMA`/`__builtin_fma` vs `HAVE_FAST_FMA`/`fma`,
  `math_narrow_eval`/`math_opt_barrier`/`math_force_eval` vs `eval_as_double`/`opt_barrier_double`/
  `force_eval_double`, `__glibc_unlikely` vs `unlikely`), `#if` branches for other table sizes and
  polynomial orders, an `opt_barrier_double(x)` around the subnormal normalization (no value
  effect) and the symbol aliases. Tables: the 128 `pow_log_data` entries, `ln2hi`/`ln2lo`, the 7
  log coefficients, `invln2N`, `negln2hiN`/`negln2loN`, `shift`, the 4 exp coefficients of N = 128
  and the 256 `exp_data.tab` words are **equal value by value** (checked by the generator script).
  `errno` handling (`WANT_ERRNO`, `check_oflow`/`check_uflow`) never changes a value.
- The later upstream commit `d9887682bf` (2025-10-28, "Fix pow special case") changes the non-FMA
  `specialcase` only when rounding upwards; it is not in glibc 2.39 and is not ported.
- The tables were generated from the pinned C files by a scratch script (not committed): each
  hex-float literal written as `f64::from_bits(0x…)` with the literal kept in a comment, scaled
  coefficients (`0x1.555555555556p-2 * -2`) multiplied exactly. The padding field of the log table
  (always 0) is dropped.
- Wrapper: V8 `13.6.233.17` `src/numbers/ieee754.cc` `math::pow`, the `use_std_math_pow` branch,
  verbatim: y NaN → NaN; y ±∞ and x = ±1 → NaN; x NaN → quiet NaN; y = 2 → `x * x`; y = 0.5 → +∞ if
  x is ±∞ else `sqrt(x + 0)`; else the core. (Its comment says the two shortcuts match the
  optimizing compilers; the tier check below agrees.)
- Port rules followed: C operation order kept (clippy's `assign_op_pattern` suggestion for
  `lo = one - hi + y + lo`, `lo += …`, would reorder the sum: written as a new binding), no
  `mul_add`, C unsigned arithmetic with `wrapping_*`, `(int64_t) tmp >> 52` as an `i64` shift.
  `__math_invalid` returns `f64::NAN` (the C `(x - x) / (x - x)` gives a NaN whose payload is not
  observable after canonicalization).

### Parity (bit-exact)

Capture `tests/captures/pow.capture` (132 KB): 8 distributions of 10^7 pairs (seeds
`0x706f7701`…`08`): `gamma` x ∈ [0, 2) × y ∈ {2.4, 1/2.4, 2.2, 1/2.2, 3, −1, 10, 0.5, 2}; `range`
x ∈ [0, 100) × y ∈ [−5, 5); `inty` x ∈ (0, 1e3] × integer y ∈ [−20, 20]; `negint` x ∈ [−1e3, 0) ×
integer y ∈ [−40, 40]; `near1` x ∈ 1 ± 2^-20 × y ∈ ±2^30; `wide` x ∈ [0.25, 4) × y ∈ [−1100,
1100) (overflow, underflow, subnormal results, `specialcase`); `subx` subnormal or zero x of both
signs × y ∈ [−2, 2); `bits` random bits². Each has `in5` (digest of the first 10^5 inputs, so the
Rust generator is checked apart from pow), `n5` and `n7`. Plus 76 special values with every pair
(5,776 results written out) and `pow(10, n)` for n ∈ [−350, 350] written out with "equals
`Number('1e' + n)`". Tiers: default and `--no-opt --no-maglev` give identical facets
(`@tiers-agree yes`).

| Check | Result |
|---|---|
| native Windows x64, `cargo test --release -p uo-jsnum --test pow -- --include-ignored` | **80,000,000 / 80,000,000** random pairs, 5,776 / 5,776 special pairs, 701 / 701 powers of ten: equal |
| wasm32-unknown-unknown (a scratch cdylib exporting the crate's `pow`: 7,278 bytes, no imports, valid for the Safari 15 feature set with `wasm-tools validate`) via `node js/capture-pow.mjs --verify-wasm <file.wasm>` | **1,510 / 1,510** capture facets equal (every n5 and n7 digest, the specials, the powers of ten) |
| `Math.pow(10, n) !== Number('1e' + n)` for n ∈ [−350, 350] | reference and Rust: n ∈ {23, 126, 210}; default Windows Node: {23}; glibc FMA: {23, 210} |
| TS call sites (S2): `pow(10, -4) === 1e-4`, `pow(10, -5) === 1e-5`, `7.5 / pow(10, -5)` = `7.5 / 1e-5`, `pow(10, n)` = the literal for n ∈ [−22, 22] | hold (`tests/pow.rs` `ts_call_sites`) |

`uo-wasm`'s `jsnum-selftest` export is not part of this stage; until it exists, `--verify-wasm`
takes any module exporting `pow(f64, f64) -> f64` (the verifiers can point it at the uo-wasm
build).

### The cost of (a): differences from default Windows Node

Same 8 × 10^7 inputs, Node 24.15.0 win-x64 (UCRT) vs the reference: **55,494 / 80,000,000
(0.069 %)**, all 1 ulp (gamma 0.074 %, range 0.098 %, inty 0.092 %, negint 0.095 %, near1 0.094 %,
wide 0.076 %, subx 0.024 %, bits 0.0006 %); in the written-out sets 9 values differ (among them
`pow(10, 126)` and `pow(10, 210)`). For comparison: glibc FMA vs Windows 0.048 %, glibc FMA vs the
reference 0.049 %. This is the 0.05–0.08 % the plan expected.

## Stage 3 — port (increment A)

Porter: Sonnet 5.5 (general-purpose agent), 2026-11-01/02, in the worktree `rust-core`. Everything of
increment A except `pow` (Opus, previous section). Gate 2 is not requested here.

### What was ported

| Rust | V8 13.6.233.17 source | Notes |
|---|---|---|
| `fdlibm/rem_pio2.rs`: `rem_pio2`, `kernel_rem_pio2` | `ieee754.cc` 118, 443 | `prec` fixed to 2 (the only caller, `jk = 4`): the dead `prec` 0, 1, 3 branches of the final switch are not carried over |
| `fdlibm/kernels.rs`: `kernel_cos` (`qx` variant), `kernel_sin`, `kernel_tan` | 305, 673, 733 | |
| `fdlibm/trig.rs`: `acos`, `asin`, `atan`, `atan2`, `cos`, `sin`, `tan` | 862, 991, 1116, 1228, 1352, 2448, 2510 | the non-glibc branches |
| `fdlibm/explog.rs`: `exp`, `expm1`, `log`, `log1p`, `log2` (+ `k_log1p`), `log10` | 1447, 2213, 1638, 1783, 1941/1951, 1973 | |
| `fdlibm/hyper.rs`: `atanh`, `cosh`, `sinh`, `tanh` | 1557, 2554, 2930, 2986 | |
| `fdlibm/cbrt.rs`: `cbrt` | 2333 | |
| `fdlibm/mod.rs`: word helpers (`high_word`, `low_word`, `from_words`, `set_*_word`), `scalbn` (fdlibm `s_scalbn.c`, the C library function `rem_pio2` calls), the `consts!` macro | 41–113 | every constant is `f64::from_bits` of the two hex words of the C comment; a unit test checks each against the decimal literal of the source (about 130 constants) |
| `math/hypot.rs`: `hypot`, `hypot3`, `hypot_n` | `math.tq` 399–510 | Infinity before NaN; 3 arguments with the compensation term; more than 3 Kahan |
| `math/mod.rs`: `round`, `sign`, `max`, `min`, `max_n`, `min_n`, `imul`, `clz32`, `fround`, `floor`, `ceil`, `trunc`, `abs`, `sqrt`, `rem`; re-exports of the 18 fdlibm functions, `hypot*`, `pow` | ECMA-262, `Float64Round` | `mod pow; pub use pow::pow;` as laid out |
| `conv.rs`: `to_int32`/`uint32`/`int16`/`uint16`/`int8`/`uint8`/`uint8_clamp`, `js_index`, `canonicalize_nan`, `truthy`, `is_integer`, `is_safe_integer` | ECMA-262 | |
| `text.rs`: `is_js_space`, `js_space_len`, `trim`/`trim_start`/`trim_end`, `split_js_space`, `utf16_len`, `utf16_slice` | ECMA-262; the set and `js_space_len` copied from `uo-opc` `xml/chars.rs` | `uo-opc` is not switched to it yet (G7, after gate 2) |
| `parse.rs`: `to_number`, `parse_int` | `conversions.cc` `InternalStringToDouble` (692), `InternalStringToIntDouble` (192–290), `StringToIntHelper::DetectRadixInternal`, `NumberParseIntHelper` (487–700) | see decisions 6 |

About 2,700 lines of Rust in `src` (pow included). `asinh` and `acosh` are not ported (not in the API). `LICENSE-V8` holds
the V8 BSD-3-Clause text and the Sun fdlibm notice; the crate `license` is `MIT AND BSD-3-Clause` (G3).

### Tests and results

Capture: `js/capture-jsnum.mjs` (deterministic: one FNV-1a seed per record, no clock, no randomness) writes
`tests/captures/jsnum.capture` (645 KB) with `@platform`, and runs everything a second time as a child with
`--no-opt --no-maglev`, failing unless the 10^5 digests agree (`@tiers-agree 121 records`).
`tests/captures/inputs.txt` is the one shared data file (98 special values, 68 high-word thresholds: every
8-hex-digit constant outside comments of `ieee754.cc`, minus legacy pow and the masks). Generator run time: about 2 min.

| Test file | What | Result |
|---|---|---|
| `tests/differential.rs` | 121 numeric records (every function over the plan's distributions plus `bits`): digest of the outputs at 10^5 in `cargo test` and at n_full (10^7; 10^6 for `%`) with `--include-ignored` | **all 121 records bit-for-bit at both sizes: 1,192,000,000 samples**, on the first replay (the oracle found no port bug); the 10^5 set also runs in debug with overflow checks |
| `tests/special.rs` | 46 records written in full (31,194 outputs: the specials, the thresholds with high word − 1 and 0, low words 0, 1, 0xFFFFFFFF, both signs, all pairs of the first 40 specials, triples, 4-tuples, `max_n`/`min_n` lists), plus the JS results written by hand | 46/46 identical |
| `tests/strings.rs` | corpus of 199 strings with `Number`, `parseInt` in 14 radixes, `trim*`, `split(/\s+/)`, `length` (3,980 comparisons); the white space set over every code point (25 chars; `js_space_len` for all); 17 random-string records (10^5 and 10^6 strings; 10^4 and 10^5 for the `utf16_slice` text record, every cut position); the S9 cases by hand | 0 mismatches |
| `tests/constants.rs` | the six `core::f64::consts` that exist in JS have the bits of `Math.PI`, `E`, `LN2`, `LN10`, `SQRT2`, `SQRT1_2` | ok |
| `tests/memory.rs` | M1: every numeric function and every parser allocates 0 bytes (`measure`, asserted); a cut-surrogate `utf16_slice` frees what it allocates (`assert_no_leak`, at most 64 B); M2: soak of 1,000 iterations × 800 batches (9,600 calls of every function), tolerance 0, ends at the starting value | ok (`m2_soak_1000` is `#[ignore]`d, 3 s in release) |
| unit (`fdlibm/mod.rs`) | hex words = decimal literals; `scalbn` identities | ok |

The replay is sensitive: adding 1e-12 to `S2` in `kernel_sin` made 6 of 208 facets fail (source restored).
Cross-OS check (the plan's "the 19 fdlibm functions give identical digests on win-x64 and linux-x64"): the same generator run in WSL
Ubuntu-24.04 with Node v24.15.0 linux-x64 (glibc 2.39, FMA CPU, no tunables) wrote a capture identical to the Windows one
line for line apart from `@platform` (all 121 digests at 10^5 and 10^7, the specials, the corpus, the random-string digests).
The committed capture is the Windows one (`os=win32 arch=x64`, Node v24.15.0, V8 13.6.233.17-node.48, no flags).

`cargo test --release -p uo-jsnum -- --include-ignored` (pow included): green. `cargo clippy -p uo-jsnum --all-targets
--all-features -- -D warnings`, also with `--target wasm32-unknown-unknown`: clean. `cargo build -p uo-jsnum
--target wasm32-unknown-unknown` (dev and release): ok (release rlib 258 KB before pruning; the real contribution is a stage-4
`twiggy` measurement). Not run: Miri (no `unsafe`, forbidden) and `cargo deny` (not installed here).

### Enforcement (G5, G6)

- `core/clippy.toml`: `disallowed-methods` with 88 entries: the 39 `f64` methods of the plan (including `to_degrees`,
  `to_radians`, `midpoint`, `abs_sub`), the same 39 on `f32`, and the 10 whitespace methods of `str`/`char`/`u8`. Each reason names
  the replacement.
- `core/tests/clippy-ban/` (outside the workspace) calls every listed path once; `check.sh` asserts clippy reports exactly as many
  `clippy::disallowed_methods` diagnostics as `clippy.toml` has entries: **88 of 88**. Run in the `lint` CI job.
- `core/tests/float-casts.sh` (G5): clippy with `cast_possible_truncation` and `cast_sign_loss` as JSON, failing only on `f32`/`f64`
  sources, on the library code of the engine crates (all but `uo-jsnum`, `uo-testkit`, `uo-alloc`). `core/tests/grep-bans.sh`: float
  `parse` outside `uo-jsnum` and the `consts` other than the six.
- `core/deny.toml`: `[bans]` on `libm` and `num-traits` (neither is in a lock file). CI job `deny`: `cargo deny check bans`.
- `.github/workflows/rust-core.yml`: wasm clippy, the three scripts, the `jsnum` job (`--include-ignored`) and `deny`.
- Opt-ins in existing code: `uo-opc/examples/opc_bench.rs` (`f64::min` on timings, module-level `#![expect]`) and
  `uo-opc/tests/xml_strict.rs` (`char::is_ascii_whitespace`: the oracle splits at XML white space; `#[expect]` on `sdump`).
  Clippy was verified clean with `-p uo-jsnum -p uo-testkit -p uo-alloc -p uo-opc -p uo-wasm -p uo-ffi` and in `fuzz`, and
  `cargo test --release -p uo-opc` is green. `uo-fonts` was mid-edit by other agents: the ban list reports
  4 × `str::trim_start` in `uo-fonts/src/catalog.rs` (123, 125, 137, 139), `float-casts.sh` reports `(x as i64) as u8` in
  `uo-fonts/src/js.rs:122` (`uo_jsnum::conv::to_uint8` is that function) and `grep-bans.sh` reports `s.parse::<f64>()` in a
  test helper of `js.rs:164`. Those are for the uo-fonts agents (opt-in with a reason, or use `uo-jsnum`); its S15 keeps a private `js` module.

### Decisions taken outside the plan (the model comparison depends on them)

1. **Captures of the non-pow functions ran on Windows x64**, as briefed. The plan's capture-machine row (G1 (a)) says every math
   capture runs in WSL; that is needed for `pow` only. `Capture::require_platform` takes a `PlatformSpec`; this crate's tests ask for
   V8 `13.6.233.17`, `arch=x64` and Node `v24.15.0`, no OS; `tests/pow.rs` asks for the Linux/glibc/hwcaps spec. The cross-OS identity
   above is the evidence.
2. **uo-testkit changes** (the plan's `@platform`): `js/testkit.mjs` `platform()` and the automatic `@platform` line of
   `CaptureWriter` (`features=` is the `fma`/`avx2`/`avx512f` flags of `/proc/cpuinfo` on Linux, `n/a` elsewhere); `Stream.digest()`
   (non-destructive, like Rust); `Stream.f64` no longer builds the hex text unless the stream traces (same bytes, much faster);
   Rust `Platform`, `PlatformSpec`, `Capture::platform`, `Capture::require_platform`; one test in `tests/kit.rs`.
3. **Samples and seeds**: seed = FNV-1a of the record key (recorded as the `seed` facet and checked); one pass keeps the digest at
   10^5 (`small`) and at n_full (`full`). Integer-valued outputs (conversions, predicates, `js_index`, `clz32`) are hashed as the f64
   number, so there is one stream item kind.
4. **Two tables, one oracle**: the generator and `tests/support/mod.rs` hold the same table of records, generators and operations;
   a transcription error would show as a digest mismatch (none occurred). The string corpus lives only in the generator: its
   inputs are written in the capture (`s`, JSON) and decoded by the Rust test.
5. **`js_index`**: `−0` is `Some(0)`; NaN, fractions, negatives and `≥ len` are `None`; confirmed by `Float64Array` reads with a number key.
6. **`parse_int` radix 10** returns infinity above 310 significant digits without parsing (V8 keeps the first 310 digits, at least 1e309)
   and parses shorter runs with `str::parse::<f64>` (correctly rounded, like `Strtod`). **`to_number`** validates the ECMA-262 grammar by hand
   and then calls the same parse on the validated slice (checked: `1e99999999999999999999`, `0e…`, `5.`, `.5`, `1.e3`, 10 MB of digits).
   The power-of-two integer rounding is a literal port of `InternalStringToIntDouble` with the digit run chosen by the callers (its junk
   branches are the callers' scans).
7. **`to_int32` and the other ToIntN** use the exponent/significand shift of V8's `DoubleToInt32`, not `fmod` (no libm call on wasm).
8. **NaN results** are `f64::NAN` (quiet) where C returns `signaling_NaN()`; payloads are canonicalized before every comparison anyway.
9. **File layout**: `src/math/{mod,hypot,pow}.rs` (brief) instead of `src/math.rs`; the fdlibm split in `rem_pio2/kernels/trig/explog/hyper/cbrt`;
   the 18 fdlibm functions are `pub` in a private module and re-exported by `math`. `fdlibm/mod.rs` allows `clippy::eq_op`
   (`x - x` makes the NaN) and `needless_range_loop` (the C index loops stay as written).
10. **Special-value thresholds** are every 8-hex-digit constant of the C file (68), with `hi − 1` and `hi`; only the 17 fdlibm unary
    functions get them, the exactly-defined ones get the specials only. Binary functions use the first 40 specials (1,600 pairs).
11. **The tier check** compares the 10^5 digests of a `--no-opt --no-maglev` child; the 10^7 digests are taken in the default tiers only.
12. **The count test is a script** (`check.sh`), not a `cargo test`: it needs a clippy run on a crate outside the workspace.

### Not done

- `uo-wasm` feature `jsnum-selftest` and `tests/jsnum.test.mjs` (native = wasm = capture, Node and WebKit): stage 4.
- Fuzz targets `jsnum_parse`/`jsnum_math` (security plan, stage 4); the 10 MB-digit case is a smoke test in `tests/strings.rs`, not a timing bound.
- wasm size measurement (`twiggy`) and M3 numbers other than "0 bytes" (stage 4).
- `uo-opc` switching to `uo_jsnum::text` (G7, after gate 2); a `cargo deny` run (tool not installed).
- `UO_JSNUM_DUMP` exists in the generator and in `tests/differential.rs` but was never exercised on a real mismatch (there was none).

## Stage 4 — memory (increment A)

Memory verifier: Sonnet 5.5 (general-purpose agent), 2026-11-02, worktree `rust-core`. Everything below comes from
tests that ran (`CARGO_TARGET_DIR=core/target/jsnum-mem`, Windows x64, rustc 1.97, Node 24.15, Playwright WebKit 26.5);
nothing is judged by reading. No leak, no unbounded growth and no allocation outside `utf16_slice` was found; no code of
the crate was changed.

Tests added (the porter's `tests/memory.rs` is untouched): `core/crates/uo-jsnum/tests/memory_gate.rs` (native) and
`core/crates/uo-wasm/tests/jsnum_memory.test.mjs` (Node and WebKit) with the exports of `core/crates/uo-wasm/src/jsnum_memtest.rs`
behind the feature `jsnum-memtest` (implies `count`; never shipped). `browser/run.mjs` forwards `UO_JSNUM_SOAK` and `UO_WASM_PATH`.
Run: `cargo test --release -p uo-jsnum --test memory --test memory_gate -- --include-ignored`; `cargo build -p uo-wasm
--features jsnum-memtest --target wasm32-unknown-unknown --profile wasm`, then `node --test crates/uo-wasm/tests/jsnum_memory.test.mjs`
and `node crates/uo-wasm/tests/browser/run.mjs --browser webkit crates/uo-wasm/tests/jsnum_memory.test.mjs`.

### M1 — no allocation, no leak per operation

| Claim | Test | Result |
|---|---|---|
| Every public function is measured | `m1_covers_every_public_function`: scans `src` for the 59 `pub fn` (41 own + 18 re-exported fdlibm) and fails unless each is called module-qualified in `memory.rs` or `memory_gate.rs` | 59/59 |
| Every numeric function allocates 0 B (and peaks at 0 B) | `m1_every_function_allocates_nothing`: each call under its own `measure`, inputs and outputs `black_box`ed, 22 inputs (±0, subnormals, NaN, ±∞, 2^31, 2^32+0.5, 2^53+1, 1e21, MAX…): 22 × 22 pairs for the 8 binary functions, 22 for each unary one, `hypot_n`/`max_n`/`min_n` on empty, 4-element and 10,000-element slices | 0 B in every call |
| Parsers and text functions allocate 0 B on every path | `m1_text_and_parsers_allocate_nothing_on_every_path`: 34 strings (empty, white space only, junk, `Infinity`, `0x`/`0o`/`0b`, `1e`, overflow, lone prefix, emoji, cut pairs) × `to_number`, `parse_int` with 17 radixes (0, 1, 2, 3, 7, 8, 10, 16, 32, 36, 37, 100, −1, −36, 2^21, `i32::MIN`/`MAX`), `trim*`, `utf16_len`, `split_js_space`, `js_space_len` at every byte offset, `utf16_slice` over 11 × 11 start/end values including `i64::MIN`/`MAX` and every cut position | 0 B; every `utf16_slice` result that is `Cow::Borrowed` allocated 0 B |
| Huge inputs | `m1_parsers_on_huge_and_hostile_strings_allocate_nothing`: 18 inputs of 10 MB (digits, zeros, spaces then 1, U+3000 then junk, `0x`/`0b`/`0o` + 10 MB, `1e` + 5 MB of 9, `1.` + 5 MB of 0 + `e-99`, 5 MB . 5 MB of 9, `z` and `6` for radix 36/7, `-`, U+00E9, U+1F600, `a `, digits then `!`) × `to_number`, `parse_int` in 10 radixes, `trim`, `split_js_space`, `utf16_len`, borrowed `utf16_slice` | 0 B, peak 0 B, all 18 × 14 calls |
| The allocating path frees what it allocates | `m1_cut_surrogate_slice_allocates_only_its_output_and_frees_it` (porter, `assert_no_leak`) and the M3 test below, which also asserts `live_delta` | live bytes return exactly |
| The harness sees allocations in release | the owned `utf16_slice` calls above are measured in `--release` with the output `black_box`ed and report 6 B to 4,000,003 B: LLVM does not elide them, so a 0 elsewhere is a real 0. The porter's `m1_numeric_functions_allocate_nothing` discards its result, so on its own it would not prove this; the new tests do | ok |
| wasm | `jsnum_memory.test.mjs` M1: 100 numeric batches (every numeric function) and 37 strings (up to 32 KB) × 7 kinds (`to_number`, `parse_int` 10/7/16/36, `split_js_space`, `trim`): the module's total allocated bytes (`uo_alloc::global_stats().0`) do not move; the all-strings export (owns one cut-surrogate `String`) leaves live bytes unchanged | 0 B in Node and WebKit |

The only allocation in the crate is `utf16_slice` when a surrogate pair is cut (`String::with_capacity(middle + 6)`).

### M2 — soak

| Soak | Iterations | Result |
|---|---|---|
| native, mixed batch (porter's `m2_soak_1000`) | 1,000 × 800 batches = 9,600 calls of every function and the string functions, sampled every 50 | tolerance 0 B, series constant, back to the start after `clear_caches()` |
| native, hostile strings (`m2_soak_hostile_1000`, `#[ignore]`d, 1.5 s in release; `m2_soak_hostile_short` runs in `cargo test`) | 1,000 × 8 strings of 64 KB (digits, `z`, spaces then 1, `0x`+f, `1e`+9, emoji, `a `, empty) × `to_number`, `parse_int` 0/7/36/99, `trim`, `split_js_space`, `utf16_len`, two `utf16_slice` | one distinct sample (524,676 B = the 8 inputs), tolerance 0 |
| wasm (Node 24.15 and WebKit 26.5) | 1,000 iterations of 20 numeric batches + 37 strings (229,562 input bytes), sampled every 50 | live bytes 229,562 B (the input buffers) in every sample; `memory.buffer.byteLength` 1,441,792 B (22 pages) in every sample; Node 2.2 s, WebKit 6.3 s |

wasm high-water mark: the module starts with 1,114,112 B (1 MiB of stack + data) and 0 live bytes; 1,000 numeric batches
alone leave it at 1,114,112 B, so the crate adds nothing to the high-water mark. The 328 KB above it in the soak is the
229 KB of test inputs plus the allocator's bookkeeping, and does not grow. For a phone: the crate itself needs no heap and
under 2 KB of stack.

### M3 — budgets (first record: the baseline for later increments)

| Function | Allocated | Peak above start |
|---|---|---|
| every numeric function, `to_number`, `parse_int`, `trim*`, `split_js_space`, `utf16_len`, `js_space_len`, `utf16_slice` without a cut pair | 0 B, on inputs of 10 MB | 0 B |
| `utf16_slice` cutting a pair at the start and/or the end (`m3_utf16_slice_owned_output_is_the_output_plus_at_most_three_bytes`) | output ≤ allocated ≤ output + 3 B: one allocation of `middle + 6` B (both cuts: exactly the output). Measured: input 10 B → 6 B (both cuts) and 11 B (one cut); 4,002 B → 3,998 B and 4,003 B; 4,000,002 B → 3,999,998 B and 4,000,003 B | the same single allocation (no growth, no realloc), i.e. at most 1 B per input byte + 3 B; the plan's bound (peak ≤ 2 × output) holds with room |

Stack of the deepest functions on wasm32 (shadow stack painted below the call with two patterns and scanned for the lowest
touched byte, ±16 B; 1 MiB available; `jsnum_memory.test.mjs`, same numbers in Node and WebKit):

| Function | Stack |
|---|---|
| `sin`/`cos`/`tan` (large argument: `rem_pio2` → `kernel_rem_pio2`, four 20-slot arrays) | 643 B (max over 13 arguments incl. 1e22, 1e300, ±∞, NaN, subnormal) |
| `asin`, `acos`, `atan`, `atan2`, `exp`, `expm1`, `log*`, `sinh`, `cosh`, `tanh`, `atanh`, `cbrt`, `pow`, `hypot3`, `to_int32` | 11 B (leaf code, no spills) |
| `to_number` | 1,655 B (max over 20 inputs, including a 770-digit halfway case, the 309-digit `2.2250738585072011…e-308` family and 100,000-character runs) |
| `parse_int` radix 10 / 7, 16, 36 | 907 B / 59 B |
| `split_js_space`, `utf16_slice`, `trim` | 59 B, 55 B, 59 B |

All are far under the test's bounds (4 KB numeric, 8 KB parsers) and none recurses; stack does not grow with input length
(the 100,000-character inputs are in the maximum above).

wasm size (stage-3 "not done" item): `twiggy` is not installed, so the figure comes from the function sizes of an unstripped
`wasm`-profile build of `uo-wasm --features jsnum-memtest` (throwaway script): functions named `uo_jsnum::*` add up to 33,123 B
of code (`rem_pio2` 5,400 B, `utf16_slice` 1,558, `pow` 1,519, `cos` 1,240, `sin` 1,116, `to_number` 1,069, `hypot_n` 1,061,
`int_double` 899, `expm1` 867, `parse_int` 850, `log1p` 814, `acos` 710), plus about 6 KB of core's `f64` parser
(`dec2flt`) and the tables (log 128 × 32 B + exp 256 × 8 B for `pow`, `TWO_OVER_PI`, `NPIO2_HW`: about 7 KB). The whole build is 80,020 B
against 15,912 B without the feature (+64,108 B raw, +35,950 B gzip), which includes the harness (it calls everything, with inlined
copies) and the counting allocator. So the crate costs about 45 KB raw in a real module, above the plan's 25–35 KB estimate
for the fdlibm functions alone because `pow`, the parsers and the tables are in it; a `twiggy` figure per increment stays
for gate 3.

### M4, M5, M6

- **M4**: no handle crosses a boundary; the wasm exports take and return numbers and a borrowed buffer that JS frees with `uo_free_bytes`
  (checked: live bytes return to the starting value after the buffers are released).
- **M5** — no caches, proved by `m5_no_caches_no_interior_mutability`: the 16 source files contain no `thread_local!`, `OnceLock`,
  `OnceCell`, `LazyLock`, `Mutex`, `RwLock`, `RefCell`, `Cell<`, `Atomic*`, `static mut`, `Box::leak`, `mem::forget`, `HashMap`,
  `BTreeMap`, `Rc`/`Arc`; the only `static` items are the four immutable number tables of `pow` (`exp_data.rs` 2, `log_data.rs` 2). With the
  M2 soaks (tolerance 0 over thousands of calls on distinct hostile inputs) a cache that fills would show.
- **M6**: `#![forbid(unsafe_code)]`, no `unsafe` in `src` (grep: only the `forbid` attribute itself): no Miri job. The test-only wasm exports use `unsafe` for the borrowed buffer
  (documented SAFETY comment); they are not part of the crate.

### Findings

None in the crate. Notes for the maintainer:

1. The porter's `m1_numeric_functions_allocate_nothing` ignores the measured result (`let (_, usage) = measure(...)`) and its batch folds
   into an `acc` that nothing reads; it is sound only because the functions have no allocation to elide. `memory_gate.rs` repeats the claim
   per function with `black_box` on inputs and outputs, so it does not depend on that.
2. The plan's M2 says 10^4 calls of every function per iteration; the porter's soak makes 9,600 (12 inputs × 800 batches). Same order; not changed.
3. `m1_parsers_on_huge_and_hostile_strings_allocate_nothing` builds about 100 MB of inputs and takes 21 s in a debug build (1 s in
   release): the CI `jsnum` job runs release.
4. Not measured: Chromium in the browser run (Playwright's Chromium is not installed here; Node and WebKit ran); real Safari/iOS (gate 3,
   BrowserStack); native stack depth (wasm32 is the constrained target); a `dhat` profile (nothing grows, so there is nothing to explain).

## Stage 4 — parity (increment A)

Parity verifier: Sonnet 5.5 (general-purpose agent), 2026-11-02, worktree `rust-core` (HEAD a1f07b9c plus the uncommitted work of the other stage 4 agents),
Windows x64 (i5-14600KF, FMA), Node v24.15.0 / V8 13.6.233.17-node.48, WSL Ubuntu-24.04 (glibc 2.39), rustc 1.97.1, Playwright WebKit 26.5.
`CARGO_TARGET_DIR=core/target/jsnum-parity`. **Result: 0 mismatches between the Rust port and V8 on everything the porter captured and on everything added
here; 0 port bugs, so 0 parity rounds.** Two verifier-side findings (a coverage gap and one bug of the verifier's own test model) are listed below.

### Re-run of the porter's claims

| Check | Result |
|---|---|
| `node js/capture-jsnum.mjs --out <file>` on this machine (Windows x64, 2 min 15 s) | **byte-identical** to the committed `jsnum.capture` (121 digest records, 46 special records, 199 corpus strings, the tier check inside) |
| the same generator in WSL, Node 24.15.0 linux-x64, glibc 2.39, once without and once with `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2` | both identical to the Windows capture **line for line except `@platform`** (decision 1 below) |
| `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2 node js/capture-pow.mjs` in WSL (1 min 8 s) | **byte-identical** to the committed `pow.capture`, `@platform` line included |
| `cargo test --release -p uo-jsnum -- --include-ignored` | green: 104 numeric records (1,022,000,000 samples: the stage 3 text says 1,192,000,000, which is not what the capture holds; the 121 digest records also include 17 random-string records, 16,100,000 strings), pow 8 distributions (8 x 10^7 pairs, 5,776 special pairs, 701 powers of ten), 46 special records, the corpus, the white space set over every code point |
| the same tests in a release build with `overflow-checks` and `debug-assertions` (`CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true`, separate target dir) | green: no arithmetic overflow in the fdlibm, pow, parser or conversion code on any of the 10^9 samples, nor on the new corpora below |
| mutation check, in a scratch copy of the crate (the real sources were never touched): 1 hypot3 compensation term dropped; 2 `to_uint8_clamp` tie to odd; 3 `int_double` tie rule `>` to `>=`; 4 `int_double` sticky bit ignoring a non-zero tail; 5 `log` constant `LG3` one ulp down; 6 pow `exp` coefficient +2^-31 relative; 7 `PIO2_3` low bit | 1, 2: the 10^5 replay fails (2 and 1 records); 3: 21 corpus/random mismatches; 4: 9 of the 26 digests of the new generated families; 5: only the 10^7 replay fails (3 records); 6: 138 of 5,776 special pairs and the distributions (4 of the 5 pow tests); 7: **not** the stage 3 numeric replay at 10^7, but the new targeted distributions (3 records at 10^5, 6 at 10^7), see "Findings". Not observable by construction: `PIO2_3T`/`PIO2_2T` last bit and a one-ulp change of an `exp` polynomial coefficient of pow (0 of 8 x 10^7 pairs) |

### Added here (the gaps found while auditing the decisions)

`js/capture-jsnum-adv.mjs` writes `tests/captures/adv.capture` (483 KB; also identical when taken in WSL, and with `--no-opt --no-maglev`); `tests/adv.rs` replays it with the
generators of `tests/support/adv.rs` (draw-for-draw mirror of the script; the same code runs inside the wasm self-test). Native run: `cargo test --release -p uo-jsnum --test adv -- --include-ignored`, 4 s.

| Record | What | Size | Result |
|---|---|---|---|
| `adv/cat0`…`cat11`, `adv/mixed` | `Number(s)` and `parseInt(s, r)` for r in 0, 10, 16, 2, 8, 36, 7, 32, 3 over strings from 12 structured families: decimals with 0–330 digit runs, zero-heavy decimals with long leading zeros, the boundaries of the double range (max, max + half ulp, min normal, 2^-1074, 2^-1075 and neighbours, 2^53 + 1, with zero tails and a sticky `1`), `0x`/`0o`/`0b` runs (ties at bit 53, 0–330 digits, sign, one invalid character), `Infinity` variants, junk soup, exponent overflow and 20-digit exponents, subnormals, dot forms (`.`, `5.`, `.e3`, `1.2.3`), 305–800-digit integers around the 309/310/311-digit threshold with tails (`.5`, `e5`, `x`), separators, non-ASCII digits; every string padded with 0–3 JS white space characters on each side (all 25) and with the negatives U+0085, U+180E, U+200B, U+2060, U+00AD, NUL | 12 x 2 x 10^5 + 2 x 10^6 = 4,400,000 strings, 44,000,000 calls | **equal** (10^4/10^5 in `cargo test`, full with `--include-ignored`) |
| `hard/0000`…`0989` | the exact decimal midpoint between two neighbouring doubles, one hair above and one hair below it (BigInt-built; 60 subnormal, 40 tiny normal, 60 in 0.5–32, 40 around 2^52–2^54, 120 huge, plus max, min, 2^52, 1) with `Number(s)` and `parseInt(s, 10)` | 990 strings of up to 1,084 characters | **equal** |
| `ext/<op>/<dist>` | the 19 math functions on the arguments where the algorithms branch: near multiples of pi/2 (k up to 2^20 and up to 2^60, ±3 ulp: the second and third pass of `rem_pio2`), every threshold word of `inputs.txt` and its predecessor with random low words and both signs (17 functions, atan2 on pairs of them), multiples and half multiples of ln2 ±3 ulp, 709.7–709.8 and −745.2…−745.1, 1 ± 2^-k, −1 + 2^-k, tiny 2^-k, sqrt2 and 1/sqrt2 times 2^e ±3 ulp, powers of two ±3 ulp, exact cubes, operands scaled by 2^±1020 (hypot, hypot3, hypot_n4, atan) | 60 records x 2 x 10^6 = 120,000,000 samples | **equal** |
| `conv/exp_sweep` | `Int32Array`/`Uint32Array`/`Int16Array`/`Uint16Array`/`Int8Array`/`Uint8Array`/`Uint8ClampedArray` stores (ToIntN and ToUint8Clamp) of every biased exponent 0…2047 x 19 mantissas (the bit-52/53/32/31 boundaries, all ones, 8 random) x both signs | 77,824 values x 7 | **equal** |
| `js_index/sweep` | `ta[i]` and `ta[i] = 99` on `Float64Array` of length 0, 1, 2, 3, 20 for the 98 special values and 22 more (−0, 0.5, 1.5, 2^32, 2^53, 1e21, 5e-324, 19.5…) | 600 cases | **equal** (`−0` reads index 0) |

Tiers: the whole `adv.capture` generated under `--no-opt --no-maglev` (Ignition only, about 10 minutes at full size, all 124,400,000 samples) is identical to the default-tier capture but for `@platform` (`flags=--no-opt,--no-maglev`).

### Audit of the 12 decisions of stage 3

| # | Decision | Verdict |
|---|---|---|
| 1 | captures of the non-pow functions on Windows x64 | **verified equal**: the same generator in WSL (glibc 2.39, with and without the pow tunables) writes the same file but for `@platform`; so does `capture-jsnum-adv.mjs`. `require_platform` (V8 prefix, x64, Node v24.15.0) is what every non-pow replay checks |
| 2 | `uo-testkit` changes (`platform()`, `Stream.digest()`, `Platform`/`require_platform`) | **verified**: every replay (`differential`, `special`, `strings`, `pow`, `adv`) checks the `@platform` line through `Capture::require_platform` before comparing anything, and the wasm test reads the same files |
| 3 | seeds (FNV-1a of the key), digest at 10^5 and n_full, integer outputs hashed as f64 | **verified**: seeds are facets that the replay compares; both sides hash the same stream |
| 4 | two tables, one oracle (generators duplicated in the script and in `tests/support`) | **verified by sensitivity**: a transcription error would be a digest mismatch (none), the mutations above show every record is sensitive; the new records use the same pattern and the draw-for-draw mirror is proved by equal digests |
| 5 | `js_index`: `−0` is `Some(0)`; NaN, fractions, negatives, `>= len` are `None` | **verified equal** against `Float64Array` reads and writes (`js_index/sweep`), 600 cases, plus the 2 x 10^7 samples of the stage 3 records |
| 6a | `kernel_rem_pio2` ported for `prec = 2` only | **verified by source**: `ieee754.cc` (V8 13.6.233.17) has one call, `__ieee754_rem_pio2` line 263 with `prec = 2`; `case 0` and `case 3` of the final switch are dead, `case 1` is `case 2` |
| 6b | `to_number`: hand grammar check, then `str::parse::<f64>` | **verified equal**: 4,400,000 generated strings (families above), 990 midpoints between doubles (the inputs a not-correctly-rounded parser gets wrong), the 199 corpus strings, 16 x 10^6 random token strings; mutating `int_double`'s tie or sticky rule fails the new replay. Overflow (`1e99999999999999999999`, 20-digit exponents), `1e-400`, subnormal boundaries, `.5`/`5.`, signs, `Infinity` (case sensitive, `Infinityx` NaN), prefixes without sign, every JS white space at both ends, U+0085/U+180E/U+200B negatives: all equal |
| 6c | `parse_int` radix 10: ±Infinity above 310 digits without parsing | **verified equal**, and **equivalent by arithmetic**: V8 keeps 310 digits, and any 310-digit integer is at least 1e309, above the largest double, so 310 and 311 as the threshold are the same function; 309-digit strings that round to a finite value (`17976931348623157` + zeros, `…158`) and to infinity (`…159`, 9s) are in `adv/cat9` with 305–312 and 320/400/800 digits, with and without tails, signs and leading zeros; mutating the threshold to 308 fails the replay (7 midpoint strings, 3 generated records) |
| 6d | power-of-two radix `int_double` literal port | **verified equal**: `adv/cat3` and the generated `parseInt` radices 2, 8, 16, 32 (ties at bit 53 with sticky tails of 0–330 digits) |
| 7 | ToIntN by the exponent/significand shift (no `fmod`) | **verified equal**: 10^7 `bits` and 10^7 `r2p40` samples per function (stage 3), the 77,824-value exponent sweep above, native and wasm |
| 8 | NaN results are quiet `f64::NAN`, canonicalized before every comparison | **accepted by name** (plan rule S5: payloads are not part of parity); all replays canonicalize on both sides |
| 9 | file layout, `fdlibm` functions `pub` in a private module | not a parity matter; builds and tests pass |
| 10 | thresholds: every 8-hex-digit constant, `hi − 1` and `hi`, low word 0/1/0xFFFFFFFF; binary functions only over the first 40 specials | **verified and widened**: the same thresholds with random low words in `ext/*/thr` (2 x 10^6 per function, 17 functions) and `atan2` over all pairs of them; equal |
| 11 | tier check at 10^5 only, in the generator | **accepted by name** for the stage 3 records (the 10^7 digests are taken in the default tiers; Ignition at that size would take hours); the new capture is checked under `--no-opt --no-maglev` at full size (above) |
| 12 | the count test is a script, not a `cargo test` | not a parity matter |

### wasm parity (`jsnum-selftest`)

Feature `jsnum-selftest` of `uo-wasm` (`src/jsnum_selftest.rs`, depends on `uo-jsnum` and `uo-testkit`; never in the shipped module) includes the test sources of the crate with `#[path]`
(`tests/support/mod.rs`, `tests/support/adv.rs`, `tests/pow.rs`: one copy of every generator) and exports, over plain numbers and JS-owned buffers (`uo_alloc_bytes`):
`uo_jsnum_selftest_count/key/digest(group, index, n)` for 5 groups (numeric records, generated strings, targeted math, pow distributions, conversion sweep),
`uo_jsnum_selftest_number/parse_int/text` for strings that JS feeds in (the 199 corpus strings and the 990 midpoints). `tests/jsnum_selftest.test.mjs` compares everything with the three capture files
(`tests/pow.rs` gained two `pub fn`s for it).
Build `cargo build -p uo-wasm --features jsnum-selftest --target wasm32-unknown-unknown --profile wasm` (154,584 bytes with the testkit and the test generators; `wasm-tools validate` with the Safari 15
feature set: valid). Run `node --test crates/uo-wasm/tests/jsnum_selftest.test.mjs` and
`UO_PLAYWRIGHT=D:/Projects/OfficeNext/slides/node_modules/playwright node crates/uo-wasm/tests/browser/run.mjs crates/uo-wasm/tests/jsnum_selftest.test.mjs`, both with `UO_WASM_PATH` pointing at the build; `UO_JSNUM_FULL=1` replays the full-size digests (`browser/run.mjs` forwards it).

| Engine | 10^5 digests | Full size (`UO_JSNUM_FULL=1`) |
|---|---|---|
| Node 24.15.0 (V8 13.6, wasm) | **104 numeric + 8 pow + 13 string families + 60 math distributions + sweep: all equal**, 3,781 corpus and 1,980 midpoint comparisons equal (1.7 s) | **equal**: 208 numeric digests (10^7 samples each), 16 pow, 26 string, 120 math; 2 min 55 s |
| WebKit 26.5 (Playwright; JavaScriptCore, the engine of Safari) | same, all equal (3.8 s) | **equal**, 2 min 44 s |
| Chromium, Firefox | not run: the browsers are not installed here (Playwright reports it) | — |

Native (`cargo test --release -p uo-jsnum -- --include-ignored`), wasm in Node and wasm in WebKit therefore give the same digests, which equal the capture taken in V8.

### Findings

No port bug (**0 parity rounds**). For the model comparison: the Sonnet port needed no rework for parity; everything below is verifier-side.

1. **Coverage gap, closed (not a bug)**: the stage 3 distributions almost never reach the third pass of the `rem_pio2` argument reduction (`i > 49`: |x − n·pi/2| about 2^-49 of x, which uniform and bit-pattern draws do not produce). Perturbing `PIO2_3` by one bit passes the whole stage 3 numeric replay at 10^7 and fails `ext/sin/pio2k20s`-style records (`sin`, `cos`, `tan` on multiples of pi/2). It is the only gap shown by a mutation; the other `ext/*` distributions (ln2 multiples, near 1, scaled operands, threshold words with random low words) found nothing and stay as protection. Keep `adv.capture` in the gate (4 s native).
2. A mismatch of the verifier's own test model, not of the port: `js_index/sweep` first failed because the Rust model summed with `Iterator::sum`, which starts at −0.0 since Rust 1.81 (a JS `let sum = 0` starts at +0); `fold(0.0, …)` fixed it. Worth knowing for every engine crate that models JS sums.
3. The stage 3 text says 1,192,000,000 numeric samples; the capture holds 1,022,000,000 in 104 numeric records (+ 16,100,000 random strings in 17 records).
4. Mutations that no sample can see (so no replay can): the last bit of `PIO2_2T`/`PIO2_3T`, a one-ulp change of one pow `exp` coefficient, `i > 49` to `i > 50` (every sample near a multiple of pi/2 has `i` of 52–54).

### Not done

- Chromium and Firefox runs of the wasm self-test (not installed); real Safari/iOS (gate 3).
- No CI step for `jsnum-selftest` was added (`.github/workflows/rust-core.yml` is being edited by other agents): it needs `cargo build -p uo-wasm --features jsnum-selftest --target wasm32-unknown-unknown --profile wasm` and the two commands above, with `UO_WASM_PATH` where the default `.wasm` of the other tests differs.
- 10^7-sample records of the stage 3 capture under `--no-opt --no-maglev` (hours) and on arm64 (the plan limits parity to x64).
- `UO_JSNUM_DUMP` is still unexercised (no mismatch to explain).

## Verification (gate 2 — pending)

### Parity

Parity: **0 mismatches** on the porter's captures and on the added ones (4.4 M structured strings, 990 exact midpoints, 120 M targeted math samples, the exponent sweep), native, wasm in Node and wasm in WebKit; 0 port bugs, 0 rounds; see "Stage 4 — parity".

### Memory (measured)

| Check | Result | Numbers |
|---|---|---|
| M1 no leak per operation | pass | 59 of 59 public functions measured, 0 B allocated (also on 10 MB inputs and every error path); the one allocating path (`utf16_slice` cutting a pair) returns to 0 live bytes; `memory_gate.rs`, `jsnum_memory.test.mjs` |
| M2 soak (native) | pass | 9,600 calls of every function; 1,000 iterations on 8 hostile strings of 64 KB; tolerance 0 B, series constant |
| M2 soak (wasm) | pass | 1,000 iterations, live bytes 229,562 B (the inputs) in every sample, `byteLength` 1,441,792 B in every sample (Node 24.15, WebKit 26.5); module starts at 1,114,112 B |
| M3 budgets | baseline recorded | 0 B everywhere except `utf16_slice` with a cut pair (output ≤ allocated ≤ output + 3 B); stack ≤ 643 B numeric, ≤ 1,655 B parsers (wasm32); crate about 45 KB raw in wasm |
| M4 boundaries | n/a | no handles |
| M5 cache caps | n/a (no caches) | proved by the source scan and the soaks |
| M6 Miri / LeakSanitizer | n/a (no unsafe) | `forbid(unsafe_code)` |

### Security

One finding (J1, low), fixed with V8's own arithmetic; 0 crashes in the fuzz targets after it;
one proposal for gate 2. Details in "Stage 4 — security" below.

## Stage 4 — security (security specialist, 2026-11-02)

Review of every file of `src/` (`parse`, `text`, `conv`, `math`, `math/hypot`, `math/pow` and its
tables, `fdlibm/*`) for panics on untrusted input (indexing, slicing at char boundaries, overflow,
shifts ≥ 64), unbounded or quadratic work, disagreements between the hand grammar of `Number()`
and `str::parse::<f64>`, wasm32 `usize` truncation and table bounds; fuzz targets in `core/fuzz`.
The finding was reproduced by a failing test before the fix (a debug panic and a release value
different from Node). `#![forbid(unsafe_code)]` holds: nothing for Miri.

### Findings

| # | Severity | Input | Effect before the fix | Fix | Test (`tests/security_regressions.rs`) |
|---|---|---|---|---|---|
| J1 | low | `parse_int(s, 32)` with at least 429,496,740 digits, e.g. `"v".repeat(429_496_740)` or `"1" + "0".repeat(429_496_740)` (a JS string can hold it: V8's maximum is 2^29 − 24; a 430 MB attribute fits the `uo-opc` XML part caps of 512 MiB on wasm32 and 2 GiB native) | `int_double` (V8 `InternalStringToIntDouble`) adds 5 to an `i32` exponent per digit. The port saturated in the digit loop and added the rounding carry with a plain `+= 1`: **an overflow panic** with overflow checks (a trap of the wasm instance, an abort under .NET with a debug native build); in release the saturation gave `Infinity` where V8 gives `0` (V8's `int` wraps: measured in Node 24.15, `Infinity` at 429,496,739 digits, `0` at 429,496,740 and 429,496,741, `-0` with a minus sign), and the carry wrapped to `0` by accident. Radix 16/8/2 cannot reach it (4 bits × (2^29 − 24) < 2^31) | `wrapping_add` in both places, which is what V8's `int` does: the result equals Node at every measured length | `radix_32_exponent_wraps_like_v8` (430 MB: `#[ignore]` in the debug run, run by the release `jsnum` CI job with `--include-ignored`, 2 s, 0.5 GB) |

J1 is **rework of the stage-3 port (Sonnet)**: `saturating_add` was a deviation from V8 not
listed in "Decisions taken outside the plan", and the carry was left unchecked next to it.

### Checked, no finding

- **Panics**: no `unwrap`/`expect`/`panic` (workspace lints). Indexing: `NPIO2_HW[n − 1]` is reached
  with `n ≥ 1` and after `n < 32`; `kernel_rem_pio2` (arrays of 20 and 66) is the fdlibm bound,
  exercised for every exponent × 6 mantissas × both signs and the double closest to a multiple of
  π/2 (6381956970095103 × 2^797, the longest zero run of the reduction, bits equal to Node) in
  `trig_argument_reduction_stays_in_bounds` (debug, overflow checks); the pow tables index with
  `% N` (log: 128 entries; exp: `2·(k % 128) + 1 ≤ 255` of 256); shifts: `checkint` 0..=52,
  `to_int32` ≤ 52 right / ≤ 31 left, `scalbn` saturating. `text` slices only at positions from
  `find`/`char_indices`; `js_space_len` reads `i + 1`/`i + 2` only after `b.get(i)` succeeded
  (`i < len ≤ isize::MAX`). `parse_generic` keeps `multiplier ≤ 0xFFFFFFFF / 36`, so its `u32`
  products cannot overflow.
- **Work**: every parser and helper visits each byte a bounded number of times; `parse_int`
  radix 10 above 310 significant digits is `Infinity` without parsing; `Number()` hands
  `str::parse` only grammar-checked ASCII (linear in core's `dec2flt`). Bounded in
  `parsers_are_linear_on_long_inputs`: ten 10 MB shapes (digits, `0.000…1`, exponents of 10^7
  digits, `0x`/`0b` runs, 10 MB of white space, U+3000 runs) through `to_number` and `parse_int` in
  7 radices, and 10 MB of `split`/`trim`/`utf16_len`/`utf16_slice(i64::MIN, i64::MAX)`: 0.5 s
  release, 7 s debug for the whole test.
- **Grammar vs `str::parse`**: the fuzz reference re-implements StrNumericLiteral; every input it
  accepts gives a non-NaN result with the literal's sign, every input it rejects gives NaN,
  the exact fast path (≤ 15 digits, |e| ≤ 22) and the `0x`/`0o`/`0b` rounding are equal bit for
  bit, and Rust's shortest decimal of every fuzzed double (`{:e}`, `{}`, `{:+.17e}`, `{:E}` wrapped
  in JS spaces) parses back to the same bits.
- **wasm32**: `utf16_len as i64` and the `i64` clamps cannot truncate; `js_index` compares with
  `len as f64`, exact for any 32-bit length. On 64-bit with `len > 2^53` the rounding of
  `len as f64` can answer `None` for a valid index, never `Some` for an invalid one (no JS typed
  array is that long): observation only.
- **Allocation**: only `utf16_slice` with a cut pair (≤ the input + 6 B); no recursion.
- **Dependencies**: `cargo audit` (RustSec, 1,402 advisories, 2026-11-02): no advisory for the 10
  crates of `core/Cargo.lock` nor the 15 of `core/fuzz/Cargo.lock` (now with `uo-jsnum`).
  `cargo deny` is not installed on this machine (not installed: the `deny` CI job runs it); the
  `deny.toml` bans were checked by hand: neither lock file has `libm` or `num-traits`, and
  `uo-jsnum` has no normal dependency.

### Proposals for gate 2 (behaviour changes, not applied)

| # | Evidence | Recommendation |
|---|---|---|
| S-J1 | J1: at 429,496,740+ radix-32 digits V8's exponent wraps and `parseInt` returns `0` for a value above 10^(6×10^8); the port now does the same | keep V8's result (parity; no document has such an attribute). The alternative is `±Infinity` (the correct value, what the port's saturation intended), a divergence from Node to list in "Browsers" |

Observation for the record of G1: the Windows UCRT `atanh` is off by about 10^15 ulps near ±1
(`atanh(1 − 2^-53)` = 18.37 instead of 18.71, which V8 and the port give) — one more reason the
engines cannot take `f64` methods from the platform; the fuzz oracle uses the `log1p` identity.

### Fuzzing

Targets in `core/fuzz/src/jsnum.rs` (registered in `TARGETS`, wrapped in
`fuzz_targets/jsnum_*.rs`); references written in the fuzz crate, independent of `uo-jsnum`:

- `jsnum_parse` (selector byte + text, lossy UTF-8): `to_number` against the StrNumericLiteral
  reference above, and unchanged by surrounding JS white space; `parse_int` in radices 0..=36 and
  −1, 1, 37, `i32::MIN`, `i32::MAX`, 65,546, −16 against the ECMA-262 steps (exact: powers of two
  rounded half to even from the bit string, radix 10 up to 38 digits through `u128`, the other
  radices below 2^53; above that the class), unchanged by trailing junk; `trim*`, `split_js_space`,
  `is_js_space` against a char-by-char reference; `js_space_len` at every byte position, past the
  end and at `usize::MAX`, and on the raw (non-UTF-8) bytes; `utf16_len`; `utf16_slice` against
  `String::from_utf16_lossy` of the clamped code units for every range of strings up to 24 units
  and for `i64::MIN`, `i64::MIN + 1`, ±len ± 1, 0, ±1, `i64::MAX` on any string.
- `jsnum_math` (selector + doubles of 8 bytes; a leftover byte picks one of 32 special values):
  each value through the 17 fdlibm functions (NaN → NaN, ranges, odd/even symmetry bit for bit,
  within 2 ulps of the platform libm; 4 for `atanh` against `½·log1p(2|x|/(1 − |x|))`, 8 for
  `sinh`/`cosh`), `round`/`sign`/`floor`/`ceil`/`trunc`/`abs`/`sqrt`/`fround`/`clz32` and every
  `conv` function against the spec (ToInt32 through exact `%` on doubles), `js_index` for 7 lengths
  up to `usize::MAX`, and `Number()`/`parseInt` of the value printed by Rust; each pair through
  `pow` (ECMA-262 special cases, else 2 ulps of libm), `atan2`, `hypot` (4 ulps), `max`, `min`,
  `imul`, `%`; the list through `max_n`, `min_n`, `hypot_n` (bounds) and `hypot3`.

Seeds (`--bin seed`): the 199 strings of the parity corpus and 8 more for `jsnum_parse`; the 98
special values and 3 doubles per high-word threshold of `tests/captures/inputs.txt`, in triples,
for `jsnum_math`. The stable driver got numeric tokens (`0x`, `e-`, `Infinity`, U+3000, U+2028,
U+00A0, an astral char…).

| Target | libFuzzer (30 min, WSL, ASan, overflow checks) | Corpus after | Stable driver (Windows) |
|---|---|---|---|
| jsnum_parse | 372,028 execs (206/s: about 80 parses and up to 1,300 slices per input) | 937 | 456,451 (120 s) + 127,621 (20 s) |
| jsnum_math | run 1 stopped at an oracle false positive (below); run 2: 1,553,298 execs before the second one; run 3 (final oracle): **1,334,347 execs, 0 stops** (740/s) | 628 | 858,786 (120 s) + 1,366,803 (120 s, final oracle) |

**0 crashes of the library, 0 timeouts, 0 out-of-memory** (slowest unit < 1 s; peak RSS 621 MB,
542 MB and 553 MB). The three stops were all in the oracle, each checked against Node and kept as the
port's value: `atanh(1 − 2^-53)` (UCRT error, Windows driver), `hypot(−2.1e300, MAX)` = `MAX` in V8
and the port where glibc rounds the exact value to `∞` (V8's `FastMathHypot` is not correctly
rounded; the oracle now counts `±∞` as one ulp past `±MAX`), `sinh(709.8706054687281)` 3.5 ulps
from glibc (fdlibm squares `exp(|x|/2)` there). CI: the `fuzz` job (now "Fuzz (uo-opc, uo-jsnum,
libFuzzer)") runs every file of `fuzz_targets/`, so both targets get 60 s per push.
`src/bin/fallback.rs` used the banned `f64::max` (clippy failed on the fuzz bins since the ban
list landed): replaced by a comparison.

```
cd core/fuzz
cargo run --release --no-default-features --features fallback --bin seed
cargo +nightly fuzz run jsnum_parse -- -max_total_time=1800 -timeout=10 -rss_limit_mb=2048   # WSL
cargo run --release --no-default-features --features fallback --bin fallback -- --secs 120 jsnum_parse jsnum_math
cd .. && cargo test -p uo-jsnum --release --test security_regressions -- --include-ignored
```

Checks: `cargo fmt --check` and `cargo clippy -D warnings` clean for `uo-jsnum` (lib and
`security_regressions`, also `--target wasm32-unknown-unknown`) and for `core/fuzz`
(`--no-default-features --features fallback --all-targets`; the nightly in WSL has no clippy
component, the `libfuzzer` wrappers are the same three lines as the uo-opc ones);
`cargo test -p uo-jsnum --release`: green (all test files, including the verifiers' new ones).

## Evolution

| Change | Keeps results? | Before | After | Decision |
|---|---|---|---|---|

## Final review and benchmarks (gate 3 — pending)

## Process metrics

| Stage | Agent / model | Wall time | Rounds | Rework |
|---|---|---|---|---|
| 1 plan (+ reuse) | port-planner / Opus | 18.7 min | — | — |
| 2 skeptic | port-skeptic-analysis / Opus | 16.7 min | — | — |
| 1 revision | port-planner / Opus: S1–S15 written in | 4.2 min | — | — |
| 3 | porter: pow (Opus) | not measured (interrupted once by a usage limit); ~60 tool calls | 0 (bit-exact on the first replay) | clippy `assign_op_pattern` rewritten without reordering; `gen` identifier (reserved in edition 2024) renamed; capture switched to uo-testkit `platform()`/`require_platform` when they landed |
| 3 | porter: increment A without pow (Sonnet) | about 3 h over two sessions (one usage-limit interruption); about 110 tool calls | 0: every record was bit-exact on its first native replay; later edits were clippy and test tables only | clippy lints (`eq_op` on `x - x`, `needless_range_loop`, `needless_late_init`, `while_let_loop`, identical `if` blocks, `manual_is_multiple_of`, `collapsible_if` in the testkit `PlatformSpec::check`, found by the uo-fonts agent); a bad TOML escape in a ban reason broke the clippy config of every crate for a few minutes, and a bad Rust escape in an `#[expect]` reason broke `uo-opc` tests; invisible characters in the generator source re-escaped |
| 3 | porter: increment B (Sonnet) | not measured; about 150 tool calls | 0: every record was bit-exact on its first native replay (51 formatter records at 10^5, then 10^6/10^7; the corpora; the sweeps) | one clippy name (`foo`), two test lints (`approx_constant`, `bool_assert_comparison`), a CRLF `Cargo.toml` from a Python rewrite, the M5 static count (4 to 5) and the public-function scan of `memory_gate.rs` (new `format` files, `parse_float`) updated; the first capture generator draft had raw invisible characters in string literals (re-escaped) and one distribution (`radix/r1e6`) that fed fractions to an integers-only function (renamed `int1e6`); a suspected out-of-bounds read in V8's `div` table was investigated and proved unreachable (decision 6) |
| 4 | memory verifier (Sonnet) | not measured | 0 | none in the crate; one test-only feature in `uo-wasm` and a two-variable forward in `browser/run.mjs`; `clippy::disallowed_methods` caught `str::trim`/`trim_start` in the new test (replaced by `text::trim*`); the first wasm soak with 1.8 MB inputs took 40 s and was cut to 230 KB; about 45 tool calls |
| 4 | security specialist (Opus) | about 2 h 40 min (four 30-minute libFuzzer runs in it, two in parallel); about 90 tool calls | 1 finding (J1), fixed by the specialist | J1 is rework of the stage-3 Sonnet port: `int_double` saturated the exponent (an unrecorded deviation from V8) and added the rounding carry unchecked (debug panic; release `Infinity` where V8 gives `0`); two lines, now `wrapping_add` as V8's `int`. In the fuzz crate: the pre-existing banned `f64::max` in `fallback.rs`; three oracle false positives (UCRT `atanh`, `hypot` at `MAX`, `sinh` near overflow) each checked in Node and the oracle widened |
| 4 | parity verifier (Sonnet) | not measured (about 2 h, most of it runs: three 1-3 minute captures, a 3 minute full-size wasm replay in Node and in WebKit) | 0 (no port bug) | none in the crate; test side: the start value of `Iterator::sum` (-0.0) in the `js_index` model, `clippy::manual_is_multiple_of` in the new generators, raw invisible characters in the generator source re-escaped as backslash-u sequences; shared files touched: `tests/pow.rs` (two `pub fn`), `tests/support/mod.rs` (`pub mod adv`), `browser/run.mjs` (forwards `UO_JSNUM_FULL`), `uo-wasm` `Cargo.toml` and `lib.rs` (feature `jsnum-selftest`); about 175 tool calls |

## Gate 2 — decisions (approved 2026-11-03, increment A)

Maintainer, verbatim: "Segue com os 2" — D1 keep V8's result; D2 approved.

Parity 0 mismatches (1.02e9 samples + 120e6 targeted + strings, native, Node wasm and WebKit wasm),
memory gate green (nothing allocates except owned `utf16_slice`), security J1 fixed, fuzz 0 crashes.

| id | Decision | Recommendation |
|---|---|---|
| D1 | S-J1: past 429,496,740 radix-32 digits V8 (and the port) returns 0 for a huge value; keep V8's result for parity, or return ±Infinity (correct, a listed divergence) | keep V8's result |
| D2 | after gate 2, uo-opc moves its JS text helpers to `uo_jsnum::text` and uo-fonts' private `js` module moves here (G7 of gate 1 / uo-fonts G8) | approve (stage 5 of each) |

## Stage 3 — port (increment B)

Porter: Sonnet 5.5 (general-purpose agent), 2026-11-03, worktree `rust-core`, `CARGO_TARGET_DIR=core/target/jsnum-b`. The formatters
and `parse_float` of increment B; gate 2 of B is not requested here. The shortest `Number::toString` is `ryu-js =1.0.3`
(`default-features = false`, G4); `toFixed`, `toPrecision` and `toExponential` are V8's own routines, line by line (G13).

### What was ported

V8 tag `13.6.233.17`, fetched from GitHub raw (BSD-3, `LICENSE-V8`). In this tag the digit routines live in `src/base/numbers/` and the three
entry points are named `Double{ToFixed,ToExponential,ToPrecision}StringView` (not `...CString`), in `src/numbers/conversions.cc`.

| Rust | V8 source | Lines |
|---|---|---|
| `format::to_fixed`, `to_exponential`, `to_precision`, the exponent/fixed text building (`Text` = `SimpleStringBuilder`), argument order and ranges | `conversions.cc` `DoubleToFixedStringView`, `CreateExponentialRepresentation`, `DoubleToExponentialStringView`, `DoubleToPrecisionStringView`; the NaN/Infinity/range checks of `builtins-number.cc` (`NumberPrototypeToFixed`, `ToExponential`, `ToPrecision`) | 1024, 1094, 1121, 1163; 21–142 |
| `format::to_radix_string` | `DoubleToRadixStringView` restricted to integral abs(x) < 2^53 (G14): its integer digit loop | 1230 |
| `format::{Js, to_js_string}` | `ryu_js::Buffer::format` (`DoubleToStringView`, 944, is what ryu-js reproduces) | — |
| `dtoa::double_to_ascii` | `dtoa.cc` `DoubleToAscii` | 31 |
| `dtoa::fixed_dtoa` (`UInt128`, `fill_digits*`, `dtoa_round_up`, `fill_fractionals`, `trim_zeros`, `fast_fixed_dtoa`) | `fixed-dtoa.cc` | 98–371 |
| `dtoa::fast_dtoa` (`round_weed_counted`, `fast_divmod`, `biggest_power_ten`, `digit_gen_counted`, `grisu3_counted`, `fast_dtoa_precision`) | `fast-dtoa.cc` | 155, 236, 256, 529, 674, 713 |
| `dtoa::bignum_dtoa` (`bignum_dtoa`, `generate_counted_digits`, `bignum_to_fixed`, `estimate_power`, the three `initial_scaled_start_values`, `fixup_multiply10`) | `bignum-dtoa.cc` | 16, 61, 232, 273, 330, 362–577 |
| `dtoa::bignum` | `bignum.cc` / `bignum.h` | 25–51, 180–264, 304–475, 560–700 |
| `dtoa::diy_fp`, `dtoa::double`, `dtoa::cached_powers` | `diy-fp.h/.cc`, `double.h`, `cached-powers.cc` (the 87-entry table is generated from the C source, not typed) | 12, —, 83 |
| `parse::parse_float` | ECMA-262 StrDecimalLiteral prefix (`parseFloat`), the same correctly rounded `str::parse` as `to_number` | — |

About 1,540 non-comment lines of library code (`src/dtoa` 1,230, `src/format` 260, `parse_float` 40), about 1,450 lines of tests and capture generator.
Files: `src/dtoa/{mod,double,diy_fp,cached_powers,bignum,bignum_dtoa,fixed_dtoa,fast_dtoa}.rs`, `src/format/{mod,radix}.rs`, `LICENSE-V8`,
`js/capture-jsnum-b.mjs`, `tests/captures/format.capture` (1.9 MB), `tests/{format,memory_format}.rs`, `tests/support/format.rs`.

The API is the plan's, with `Js` (`Display`, `Clone`, `Copy`, `Debug`), `RangeError` (`Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Display`, `Error`),
`to_fixed(x, digits: u32)`, `to_precision`, `to_exponential`, `to_radix_string(x, radix: u32)`, `to_js_string`, `parse::parse_float`. As in V8,
`to_fixed` checks the digits (0..=100) before the value (`NaN.toFixed(101)` throws) and `to_precision`/`to_exponential` answer NaN and the
infinities before checking theirs (`NaN.toExponential(200)` is `"NaN"`); `to_precision` accepts 1..=100, `to_exponential` 0..=100.

### Tests and results

Capture: `js/capture-jsnum-b.mjs` (deterministic: FNV-1a seeds, no clock; `@platform`; `@tiers-agree 54 records` with a `--no-opt --no-maglev` child) writes
`tests/captures/format.capture`. Taken on Windows x64, Node v24.15.0; the same script in WSL Ubuntu-24.04 (Node v24.15.0 linux-x64, glibc 2.39) wrote a
file identical line for line apart from `@platform`, and a second Windows run was byte-identical. Generator time about 50 s.

| Check | Result |
|---|---|
| `tests/format.rs` `fmt/*`: 51 records, `String(x)` on bits and on `k/10^d` (10^7 each) and 5 other distributions (10^6); `toFixed` (bits, ±1e21, ±1e6, `k/10^d`, integers to 2^53, abs(x) < 1, ties `k+0.5` at d = 0, dyadic ties `k/2^j` at d = j-1, and the constant digit counts 0, 1, 2, 3, 6, 20, 21, 100) over random digits arguments 0..=100; `toPrecision` (random 1..=100, ties at p = digits of k, constant p = 1, 2, 12, 15, 17, 21, 100); `toExponential` (random 0..=100, ties, constant d = 0, 1, 14, 20, 100); `toString(radix)` on 10^6 integers in (-2^53, 2^53) and in (-10^6, 10^6) with random radix 2..=36 | **equal at 10^5 (`cargo test`) and at the full size (`--include-ignored`): 69 x 10^6 formatter calls**, native, 4 s in release |
| `edge/*`: 102 written-out values (±0, ±1, ties .5, 1.005, 2.675, 8.345, 10.235, the carries 9.5, 99.5, 999999.5, the 1e-7/1e-6 and 1e21/1e20 boundaries, 999999999999999868928, subnormals, MIN/MAX, 2^53 neighbours, 2^63, 2^64, NaN, ±Infinity) x every digits argument 0..=105, 1000, 4294967295 x 3 methods; `String(x)` and its `Display`; 25 integers x radix 2..=36 | **34,025 outputs equal** (strings compared, `RangeError` where JS throws) |
| `sweep/*`: every power of two (both signs, 2^-1074..2^1023) and every 10^k (-323..308) with its two neighbouring doubles x every digits argument | **1,839,784 outputs equal** (digest) |
| `pf/*`: `parseFloat` of the 199 corpus strings of `jsnum.capture`, the 990 exact midpoints of `adv.capture` and 131 written-out strings (S9 cases, `0x10`, `1e`, `.e3`, `Infinityx`, `+-1`, 800-digit halfway cases) | **1,320 equal** |
| `rand/pf_*`: 3 alphabets (junk tokens, number tokens, number head + junk tail), 10^5 and 10^6 strings each | **equal** |
| `parse_float_by_hand`; `formatters_by_hand` (88 cases of toFixed/toPrecision/toExponential with their Node results, among them the S7 cases `(-0).toFixed(2)` = `"0.00"`, `(-1e-10).toFixed(2)` = `"-0.00"`, `(-0).toPrecision(3)`, `(0.5).toFixed(0)`, `(-0.5).toFixed(0)`, `(25).toExponential(0)` = `"3e+1"`); `number_to_string_by_hand`; `radix_by_hand` (every out-of-contract argument is `Err`) | ok |
| `no_panic_on_any_double_and_any_argument` (every biased exponent, 4 random mantissas, both signs, 15 digits arguments up to `u32::MAX`, all four formatters and `Js`) | ok, in debug (overflow and bounds checks) and in release |
| the full replay in a release build with `overflow-checks` and `debug-assertions` | equal, no panic |
| `dtoa` unit tests: `a_fast_result_is_the_exact_result` (whenever Grisu counted or the fast fixed algorithm claims a result it equals the bignum one: 100,000 random doubles x 12 digit counts, no other oracle), `fast_divmod` over 20,000 values per divisor, the `div` table against its libdivide rule, `DiyFp::times` against `Multiply`, the cached powers (consecutive exponents, exact small powers), `ten_to_the_nine_is_not_reachable` | ok |
| wasm (`uo-wasm` feature `jsnum-selftest`, groups 5-7 and the exports `uo_jsnum_selftest_format` / `_parse_float`), Node 24.15 | **equal**: 51 formatter digests at 10^5 and at the full size (26 s), 3 string records, 6 sweeps, 34,025 edge outputs, 1,320 `parseFloat` strings; WebKit not run here |
| memory (`tests/memory_format.rs`): M1 1,440 formatter calls under `measure` with inputs and outputs black-boxed: each `Ok` allocates exactly its length (capacity = length, one allocation, peak = allocated = live delta), each `Err` 0 B; `Js` formats into a stack writer with 0 B; live bytes return (`assert_no_leak`); M2 soak 1,000 x 200 batches (40,000 calls), tolerance 0, flat | ok; budgets: `to_fixed(1e20, 100)` 122 B (the longest), `to_radix_string(-(2^53-1), 2)` 54 B, `Js` 0 B |

Mutation check of the replay (each in a scratch edit, reverted): the round-half-up of `generate_counted_digits` (`>=` to `>`) failed 4 tests; the fixed round-up
condition (`point > 0` to `point > 1`) failed the digests and the sweeps; `round_weed_counted`'s `ten_kappa - unit <= unit` to `<` and its safe-round-down `>=` to `>`
passed even at 10^6 (they only make Grisu decline more or less often: the digits are the exact ones either way, so no output can show them); a worse
`divide_modulo_int_bignum` estimate (+2) and `exponent < -127` for `< -128` in the fast fixed path are equivalent mutants.

`cargo fmt -p uo-jsnum -p uo-wasm -- --check`, `cargo clippy -p uo-jsnum -p uo-wasm --all-targets --all-features -- -D warnings` (also for `wasm32-unknown-unknown`),
`cargo test --release -p uo-jsnum` (and with `--include-ignored` for `format` and the memory tests), `core/tests/grep-bans.sh`, `core/tests/float-casts.sh`,
`core/tests/clippy-ban/check.sh` (88 of 88): clean/green. wasm32 build of `uo-wasm --features jsnum-selftest --profile wasm`: 212,161 B (154,584 B before; the difference is the
formatters, `parse_float` and the new test generators; the real contribution is a stage-4 `twiggy` figure, the tool is not installed here).

### Decisions taken outside the plan (the model comparison depends on them)

1. **Layout**: `src/dtoa/` (V8's digit routines, one file per V8 file) and `src/format/{mod,radix}.rs` instead of the plan's `src/format/{mod,exact,radix}.rs`
   (`exact.rs` was the stack-bignum design that G13 replaced). The names are V8's: in this tag the entry points are `...StringView`, the helpers are in `src/base/numbers/`.
2. **Bignum**: V8's own `Bignum` (128 bigits of 28 bits = 3,584 bits, a `u32` array on the stack, about 520 B; `bignum_dtoa` holds four), not the plan's `[u32; 84]`.
3. **Not ported** (never reached by the modes in use): the shortest modes of `FastDtoa` (`Grisu3`, `DigitGen`, `RoundWeed`) and `BignumDtoa` (`GenerateShortestDigits`, the boundary deltas
   and every `need_boundary_deltas` branch), `Double::{NextDouble, NormalizedBoundaries, UpperBoundary, AsDiyFp, SignificandSizeForOrderOfMagnitude}`, `DiyFp::{Minus, Normalize}`,
   the decimal/hex string assignment, `AddUInt64`/`AddBignum`, `MultiplyByPowerOfTen` and `ToHexString` of `Bignum`, `GetCachedPowerForDecimalExponent`, `DoubleToAscii`'s
   `PRECISION` with 0 digits, `DoubleToExponentialStringView`'s `f == -1` form (the API has explicit digits, as the plan says), `DoubleToRadixStringView`'s fraction loop and its
   "unrepresented digits" loop (G14: it never runs for integers below 2^53). `fixup_multiply10` keeps the two delta bignums as the zero values V8 passes when `need_boundary_deltas` is false.
4. **Arithmetic semantics** (the J1 lesson): wherever the C code relies on unsigned wrap-around, the Rust uses `wrapping_*` (`Bignum` chunks and carries, `UInt128`, `fast_divmod`,
   `fractionals`/`w_error` of `digit_gen_counted`, the `divide_modulo_int_bignum` result); there is **no saturating arithmetic** anywhere. Plain `+ - *` on `i32`/`u64` (which panic on overflow in debug and
   wrap in release) are used where V8's preconditions make overflow impossible (`ten_kappa - rest` with `rest < ten_kappa`, digit counts, buffer positions); the full replay in a release build with
   `overflow-checks` and `debug-assertions` and the no-panic loop ran without a panic. The widening casts V8 does implicitly (`(factor as u64)` of a non-negative `int`, `result as u64` of a non-negative
   `int`) are written out.
5. **Bounds**: V8 crashes (`UNREACHABLE`) when a `Bignum` outgrows its array and reads and writes the digit `Vector` with checked `operator[]`. Here the digit buffer (`Buffer`, 128 bytes) ignores accesses outside it
   and the string builder (`Text`, 192 bytes) drops what does not fit, while `Bignum` indexes its array directly; the sizes follow from the argument ranges (121 digits for toFixed below 1e21, 100 for the others,
   a bignum below 10^342 x 2^1127 x 10^121), and the no-panic loop covers every exponent x digits argument.
6. **`div` table of `fast_divmod`**: V8's table has 9 entries (10^0..10^8) and the digit loop indexes it with the exponent of the biggest power of ten, which is 9 when the integral part reaches 10^9: an out-of-bounds read
   in the C++ if that ever happened. It never does: the largest integral part after scaling (the largest significand at every exponent, subnormals included) is 798,336,123 (`ten_to_the_nine_is_not_reachable`), so
   the entry is dead and not added; a lookup past the table takes entry 0 instead of reading out of bounds. (The first draft added a computed 10^9 entry; it was removed when the test showed it unreachable.)
7. **`DiyFp::times`** is the `__SIZEOF_INT128__` branch (what Node x64 compiles), a `u128` multiplication; `Multiply` (the portable branch) is kept under `#[cfg(test)]` to check that the two round the same. `UInt128` of `fixed-dtoa.cc`
   is ported as V8's struct, not replaced by `u128`.
8. **`Js`**: `Display` writes with `write_str`, so width, fill and precision flags (`{:>10}`) are ignored (the text is never padded or truncated); `to_js_string` allocates exactly the text (`to_owned` of the ryu-js buffer)
   instead of `Js(x).to_string()`, whose first allocation can be larger. `-0` prints `0` and NaN `NaN` (ryu-js; confirmed by the edge records).
9. **`to_radix_string`** also answers `Err` for NaN and the infinities (the plan: "integral abs(x) < 2^53, else Err") and accepts radix 10 (the digits of the integer, same as `Js` for these values). Zero (and -0) is `"0"`.
10. **`parse_float`** gets its own prefix scanner (`decimal_literal_prefix_len`, the structure of `is_unsigned_decimal_literal`) instead of refactoring `to_number`'s validator, so no increment A function changed; the literal slice goes
    to the same `str::parse::<f64>`.
11. **Digits arguments are `u32`**: JS negative or fractional digits (`toFixed(-1)`, `toFixed(2.7)`) are the caller's ToIntegerOrInfinity; `u32::MAX` and any value above 100 are `RangeError`.
12. **Tests**: the plan's "each formatter on 10^6 inputs from bits, [-1e21, 1e21], dyadic ties and k/10^d" per digits count would be 4 x 10^8 calls per formatter; each sample draws its digits argument at random instead
    (`bits_r`, `r21_r`, ...), plus records with a constant digit count (8 for toFixed, 7 for toPrecision, 5 for toExponential), the ties records (`half_*`, `dyadic_*`: digits = the tie position) and, written out or hashed,
    every digits argument for 102 hand-picked values and for every power of two and ten. `String(x)` is 10^7 on bits and on `k/10^d`, 10^6 on the other distributions. The `rand/pf_*` strings come from the token alphabets of
    `strings.rs`; `parseFloat` of the corpora reuses the strings of `jsnum.capture` and `adv.capture` (read by the script; only the results are stored in `format.capture`).
13. **Existing test files touched** (increment A tests, no test logic removed): `tests/support/mod.rs` gets `pub mod format;`; `tests/memory_gate.rs` scans `format/mod.rs` and `format/radix.rs` for public functions, accepts
    `tests/memory_format.rs` as a source of calls (the formatters allocate, so they are measured there as "exactly the output"), calls `parse::parse_float` in the two string grids, expects at least 65 functions and
    5 static tables (the 87-entry `CACHED_POWERS` joins the four of `pow`).
14. **`LICENSE-V8` was missing**: the increment A record says it holds the V8 BSD-3 text and the fdlibm notice, but the file was not in the crate. It is now (V8's `LICENSE` of the tag, the fdlibm notice of `ieee754.cc`, and
    the list of V8 files translated, including the `double-conversion` origin of `src/base/numbers/`).
15. **Lock files**: `core/Cargo.lock` has `ryu-js 1.0.3`; `core/fuzz/Cargo.lock` got the same package and the `uo-jsnum` dependency edge by hand (an offline `cargo check` there would have downgraded `libc` to the cached
    version; the file now passes `cargo check --locked`).
16. **Capture platform**: the new capture was taken on Windows x64 like most of increment A (no math function is involved: the digit routines are integer arithmetic plus one `ceil`), with the WSL linux-x64 run as the
    cross-check above; `tests/format.rs` requires V8 `13.6.233.17`, `arch=x64` and Node `v24.15.0` through `require_platform`.

### Not done

- Stage 4 of increment B: the parity verifier's own corpora, the memory verifier's wasm soak and stack depth of the formatters (the stack of the bignum path is about 3 KB by size: four 520 B bignums plus 128 + 192 + 192 B of buffers; not painted),
  `twiggy` figures, the security review, the fuzz target `jsnum_format` (plan) and `parse_float` in `jsnum_parse`.
- WebKit/Chromium/Firefox runs of the new wasm self-test; `.github/workflows/rust-core.yml` is not touched (the existing `jsnum` job runs `--include-ignored` and picks the new tests up; the self-test CI step is still the open item of increment A).
- `CHANGELOG.md`, the npm/NuGet `THIRD-PARTY-NOTICES.md` entries for ryu-js (Apache-2.0 OR BSL-1.0) and for the V8 dtoa code, and `cargo deny` (not installed here).
- `UO_JSNUM_DUMP` for the new capture (`fmt/<key>`) exists in the script and in `tests/format.rs` but was never exercised on a real mismatch (there was none).
