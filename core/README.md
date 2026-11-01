# UniversalOffice Rust core (0.10, in progress)

One implementation of the UniversalOffice engines for the browser (WebAssembly), Node and
.NET (a C# facade over the native library). Built module by module with the `rust-port`
workflow (`.claude/skills/rust-port/SKILL.md`): exact parity with the frozen TS engine of
`main`, a measured memory-leak gate, security checks and benchmarks against `main`.

## Layout

| Path | What |
|---|---|
| `crates/uo-alloc` | counting allocator and the memory gate helpers (M1–M3) used by every crate's tests |
| `crates/uo-testkit` | parity tooling: hashed canonical streams, capture files and their exact replay, seeded generators (Node side in `js/testkit.mjs`) |
| `crates/uo-ffi` | C ABI for .NET (skeleton: handles, panic guard, memory counters) |
| `crates/uo-wasm` | WebAssembly exports for JS (skeleton: handles, buffers, memory counters) |
| `dotnet/` | the .NET facade (`UniversalOffice.Native`, `SafeHandle`s) and its boundary tests |
| `fuzz/` | fuzz targets (cargo-fuzz, its own workspace; a stable driver runs them without libFuzzer) |
| `docs/modules/` | one record per ported module (`TEMPLATE.md`) |

The engine crates (`uo-opc`, `uo-fonts`, `uo-drawingml`, `uo-render`, `uo-pdf`, `uo-docx`,
`uo-docx-layout`, …) arrive one module at a time.

## Commands

```shell
cargo test --workspace --all-features            # tests, memory gate on (counting allocator)
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build -p uo-wasm --features count --target wasm32-unknown-unknown --profile wasm
node --expose-gc --test crates/uo-wasm/tests/boundary.test.mjs
dotnet test dotnet/UniversalOffice.Native.Tests  # builds uo-ffi with the counters
cargo +nightly miri test -p uo-alloc --features global --target x86_64-unknown-linux-gnu
node ../tools/bench/run.mjs --main <checkout of main>   # main × rust-core
```

Fuzzing (from `fuzz/`; Linux, or Windows with the MSVC ASAN runtime on `PATH`):

```shell
cargo run --release --no-default-features --features fallback --bin seed   # corpus/ from repo documents
cargo +nightly fuzz run <target> -- -max_total_time=600
cargo run --release --no-default-features --features fallback --bin fallback -- --secs 60   # stable, no libFuzzer
```
