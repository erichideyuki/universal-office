---
name: rust-port
description: Workflow for rewriting the UniversalOffice engines in Rust (branch rust-core, version 0.10), module by module, with parity against the frozen TS engine, measured memory-leak gates, security, reuse across Write/Sheet/Slides and benchmarks against main. Orchestrates the port specialists in .claude/agents/ with approval gates. Use for ANY work on the Rust core (core/), its bindings (wasm, FFI, the C# facade) or its benchmarks.
---

# Rust port workflow (0.10)

You, the main agent, are the **orchestrator**: you run the stages, call the specialists in
`.claude/agents/` as subagents, check what they bring back and stop at the **gates** for the
maintainer to decide.

## Fixed decisions (do not reopen without the maintainer)

- **Goal**: one implementation of the engines. Version 0.10 replaces the TS engine of Write
  (and later Sheet and Slides) and the .NET port with a Rust core. Write comes first, but
  **every module is designed for reuse by Sheet and Slides** (see "Reuse").
- **Everything moves to Rust**, including what only exists in C# today (sanitizer, templates,
  `DocxBodyWriter` and the rest of the Editing API, `.doc` conversion, Markdown/HTML export,
  the converter, Xlsx, Pptx) and the **editing model** (edit operations and undo) that lives
  in the TS editor today. The TS editor keeps only the shell: input, UI, bars, painting.
- **The TS engine and the C# port are frozen on `main`** (0.9.x): they are the parity
  reference and the benchmark baseline. No fixes or features in them until 0.10 ships.
- **Branch `rust-core`** for all of the migration; `main` is benchmarked against it regularly.
- **Plug in at once**: the browser switches to Rust when the whole `layoutDocument` path is
  ready, behind a TS/Rust switch during the transition — not stage by stage (each JS↔Rust
  crossing costs).
- **.NET**: the deliverable is a C# NuGet package that is the interface to the Rust core
  (native library per runtime in `runtimes/<rid>/native/`, P/Invoke over a C ABI; Wasmtime is
  the alternative to weigh). The package stops being "pure .NET".
- **Dependencies**: crate vs own port is decided **case by case** in each module plan
  (license MIT-compatible, maintenance, size in wasm, `no_std`/wasm support, and above all the
  effect on parity).
- **Memory leaks are a measured gate**, not an opinion: see "Memory gate".

## Layout of the code

Rust workspace in `core/` (crate names `uo-*`). A crate exists only with a reason, like the
npm packages: (1) it is used by more than one app or (2) it is a binding boundary. The rest is
a module inside the owning crate.

| Crate | Content | Today in |
|---|---|---|
| `uo-alloc` | counting allocator, memory test harness (shared by all crates' tests) | — |
| `uo-testkit` | capture/replay, exact comparison, differential document generator | experiments/rust-spike |
| `uo-opc` | zip/inflate, XML, OPC package, limits for untrusted input | docx-core package, Core OpcPackage/MiniXml |
| `uo-fonts` | sfnt, GPOS/GSUB, metrics, substitutes catalog, hyphenation patterns | shared/fonts, docx-layout/typography |
| `uo-drawingml` | presets, color modifiers, raster effects, image adjustments | shared/drawingml |
| `uo-charts` | charts and number formats (Sheet/Slides, later) | shared/charts |
| `uo-render` | the display list (types + binary encoding): the contract with the TS painter of all apps | docx-layout tree.ts |
| `uo-pdf` | PDF writer | docx-layout pdf-writer, Core Pdf |
| `uo-docx` | model, reader, writer (lossless), edit operations, undo, Editing API, exporters, `.doc` | docx-core, editor model ops, Docx/Editing |
| `uo-docx-layout` | layout engine | docx-layout |
| `uo-wasm` | bindings for browser/Node | — |
| `uo-ffi` | C ABI for .NET | — |

The C# facade lives in `dotnet/` (P/Invoke, `SafeHandle`s, the public API of the NuGet).

## Stages of a module

Work bottom-up: a module starts only when the modules it depends on passed gate 2.

1. **Plan** — `port-planner` (+ `reuse-specialist` in parallel). Scope, Rust data model
   (ownership: arenas and indices, no `Rc<RefCell>` graphs), public API of the crate, which
   crate it belongs to and what Sheet/Slides will reuse, crate-vs-port decisions, capture
   points for parity, memory budgets and soak scenario, fuzz targets.
2. **Analysis skeptic** — `port-skeptic-analysis` reads the TS (behaviour source) and the C#
   (type guide) and attacks the plan: JS semantics that do not translate, hidden state,
   missed reuse, untestable boundaries.
   **Gate 1 (maintainer): plan approved.**
3. **Port** — the whole module translated in one go (faithful: same algorithm, same order of
   floating-point operations, same results), inside the approved model. It must **compile
   with `clippy` clean** before anything else. Typescript is the source of behaviour; C# helps
   with types where TS has unions/`any`.
4. **Verification** (in parallel):
   - `parity-verifier`: 100% exact parity on the captured corpus + differential random
     documents;
   - `memory-verifier`: the memory gate (below), measured;
   - `security-specialist`: no panics on untrusted input, limits, fuzz targets running.
   **Gate 2 (maintainer): parity 100%, memory gate green, security findings handled.**
5. **Evolution** — `simplification-algorithms`: simplifications and algorithm improvements
   with parity as the safety net. Any change that alters results is listed separately with
   the oracle evidence; the maintainer decides each.
6. **Final review** — `port-skeptic-implementation` reviews the whole module; benchmarks
   `main × rust-core` (`tools/bench`).
   **Gate 3 (maintainer): findings handled, numbers accepted.**

## Memory gate (measured — a module cannot pass gate 2 without it)

All numbers come from tests that run in CI; "looks fine" is not a result.

- **M1 — no leak per operation**: tests built with `uo-alloc` (feature `count`) run each
  public operation and drop its outputs; live bytes must return **exactly** to the value
  before the operation (caches cleared first through the crate's `clear_caches()`).
- **M2 — soak**: the module's long scenario (e.g. open → layout → 100 edits → undo/redo →
  close, 1,000 times) samples live bytes every 50 iterations; after warm-up the series must be
  flat (max − min within the stated tolerance) and back to the initial value after
  `clear_caches()`. In wasm, `memory.buffer.byteLength` must stop growing after warm-up.
- **M3 — budgets**: allocated bytes and peak live bytes per operation are recorded in the
  module record; a regression over 10% against the previous record fails the gate unless
  approved.
- **M4 — boundaries**: every handle crossing to JS or .NET has a test that creates and drops
  10,000 of them (explicit `free()`/`Dispose()` and the finalizer path) and checks the
  native live bytes return; .NET uses `SafeHandle`.
- **M5 — caches**: every cache has a cap and is covered by a test that fills it past the cap.
- **M6 — tools**: `dhat` for investigation, Miri for every `unsafe` block, LeakSanitizer on
  the Linux CI for `uo-ffi` and the C# facade tests.

## Reuse (every stage, every agent)

The planner and both skeptics must answer for each module: what of this is the same in
Sheet and Slides (OPC, XML, fonts, DrawingML, charts, PDF, display list, text shaping, undo
infrastructure), and is it placed in a shared crate with an API that does not assume Word?
Placing Word-only assumptions in a shared crate, or duplicating something a shared crate
already has, is a finding.

## Records

Each module has `core/docs/modules/<crate>-<module>.md` with: scope, the approved plan,
skeptic findings and what was decided, parity results (corpus, mismatches), memory results
(M1–M5 numbers), security findings, optimizations (accepted/rejected, with numbers),
benchmarks `main × rust-core`, and the date of each gate. The gate decisions are copied
there verbatim.

## Rules for the orchestrator

- A subagent does not see the conversation: give it the module, the decisions of previous
  gates, the TS and C# files, the record path and the commands.
- Check every finding in the code before showing it; say what was discarded and why.
- Specialists of the same stage run in parallel (one message, several Agent calls).
- Repository rules still apply (`CLAUDE.md`): English in `.md`, no proprietary fonts in
  fixtures, MIT license, `CHANGELOG.md`.
