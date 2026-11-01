# Experiments behind the Rust core (0.10)

The measurements that led to the decision to rewrite the engines in Rust, kept as the
starting point of the benchmark suite (`tools/bench`, to be built in the foundation phase of
the `rust-port` workflow — see `.claude/skills/rust-port/SKILL.md`).

| Folder | What it measured |
|---|---|
| `wasm-poc/` | `UniversalOffice.Docx` (C#) compiled to WebAssembly (interpreted and AOT), its layout painted by the TS canvas painter; size, start-up, layout time against the TS engine and native .NET |
| `rust-spike/` | Spike 1: the font core (`sfnt.ts`/`gpos.ts`) ported to Rust, in TS, C# and Rust (native and wasm). Spike 2: the line breaker (`lines.ts`) ported to Rust, replayed from a capture of the TS engine over the Word oracle, CPU and memory in TS, C# and Rust |
| `perf/` | Harness that measures the editor in Chrome with the CPU slowed down (load, blocking, memory, scroll, typing latency); the profiles that led to the editor optimizations of commit d0f45bc5 |

Each folder has its own README with the results and the commands. Generated data
(`data/`, `out/`, `target/`, `web-dist/`) is not versioned.
