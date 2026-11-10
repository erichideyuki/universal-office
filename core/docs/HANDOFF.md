# Rust port — handoff (state of 2026-11-07)

Entry point for the next session that continues the Rust migration (0.10). Read this, then
`.claude/skills/rust-port/SKILL.md` (the workflow: hybrid mode, models per role, memory gate) and the module records
in `core/docs/modules/`.

## Where things are

- **Worktree** `D:\Projects\OfficeNext\.claude\worktrees\rust-core`, **branch `rust-core`**, HEAD `da31758d`
  (2026-11-07). Workspace `core/`.
- **The branch has never been pushed** (there is no `origin/rust-core`): every commit since the start of the port
  exists only on this disk. Pushing (or at least a bundle backup) is the maintainer's call — do it before anything
  risky.
- **Frozen TS reference** (behaviour source, never edit): `UO_TS_ROOT=D:/Projects/OfficeNext/.claude/worktrees/clickcompliance-docx`
  (commit `d0f45bc5`, `dist` built). Do **not** use the main checkout as TS root (its `dist` is stale, F2-20).
  Layout captures need `--accept-stale typography/font/sfnt.ts` (F7a-14) and `node --max-old-space-size=8192` past
  300 documents.
- Untracked and not needed: `core/crates/uo-docx/js/_dump.mjs` (a debug scratch of an earlier session).
- Port status in lines: `core/docs/port-status.md` (measured 49% of Write at `c3f95ae5`; **not re-measured since** —
  the estimate after this session's work is ≈ 90% of Write's TS/C# lines, ≈ 40% of the whole core with Sheet and
  Slides at 0%). Re-measure with the method at the top of that file.

## Done (commits on `rust-core`, newest last; each record has the numbers)

| Area | Commit(s) | Record | Result |
|---|---|---|---|
| D1b writer (core + W1–W6), D4 `.doc` (F0–F6), D5 export/ODF/compare, R5 follow-ups, `odraw` | `c3f95ae5`, `82c756c6`, `3d7312ef`, `ea62b4bb` | `uo-docx.md`, `uo-docx-d1b/`, `uo-docx-d5/` | parity 100%; `.doc` 179,929 equal |
| Layout F4, F6, F7a–F7d, F8 | `a7fd7d5d`, `06c22a51`, `81a4c5ef`, `4199fd8e` | `uo-docx-layout.md` | **Word oracle 2,742/2,742 documents equal end to end**, 0 mismatches; F8 41,130 facets equal |
| uo-render G (geometry, paint helpers) and E (canvas encoder, wasm paint exports) | `8ff85d49`, `2ce56e90` | `uo-render.md` | 100% facets |
| uo-pdf P (the PDF painter) | `bc62a847` | `uo-pdf.md` | byte-equal to TS `renderPdf` (300 oracle docs locally) |
| D2a session core + D2b families F1, F2, F3, F5, F6, F9, F4 (partial) | `2447379a`, `a936cdbb`, `bfb56594`, `c61f647a` | `uo-docx.md` "D2a — session (architecture)", `uo-docx-d2b/F*.md` | both harnesses (Node + Chromium) 0 mismatches |
| D3 Editing API (C# → Rust) with a C# differential harness | `54129cd7` | `uo-docx-d3.md` | 100,872 facets equal to the frozen C# |
| Milestone-1 bindings: wasm `write` viewer API, `uo-ffi` C ABI, C# facade (`core/dotnet/UniversalOffice.Docx`), server path S | `4bcf5504` | `uo-bindings.md` | first end-to-end Rust PDF byte-equal to the TS (68/68 fixtures, 211/211 oracle docs locally) |
| Docs: untrusted documents, CSP | `ff9053cc`, `9fa066ad` | `docs/security.md` | — |
| Memory benchmark `main × rust-core` | `01c72ab5` | `uo-docx-layout.md` "Benchmarks … memory" | Rust layout = **0.23–0.41×** the TS memory |
| wasm2js fallback + wasm-opt, measured | `ba3e15b9` | `uo-bindings.md` "No-WebAssembly fallback" | works under strict CSP; 4–8× slower, 1.57 MB gzip |
| wasm size items 1–3a (planner → Sonnet → reviewer) | `da31758d` | `size-optimization.md` §8 | raw −33.5%, gzip −4.1%, same output, same speed |

## Update 2026-11-07 (later the same day) — WIP finished, workspace green

The WIP below was finished in one workflow run (Opus planner → Sonnet implementer → Opus reviewer, up to 2 fix
rounds; G8 implemented on Sonnet at the maintainer's request although SKILL.md says Opus for the layout crate):

- **D2b F7** done: record `core/docs/modules/uo-docx-d2b/F7.md` (Node + Chromium harnesses, regex sites, M1–M3,
  no-panic). Declared open items there: `gradientVariations`/`lineWith`/`POSITION_PRESETS` not ported; F7-D10 (typing
  with an object selected) and some multi-selection behaviours left for the milestone-2 gate; `align_objects` with a
  `rects` length mismatch (reviewer minor).
- **G8** done: records `uo-docx-layout.md` (frontier E / G8) and `uo-bindings.md` (G8 row). Maintainer decisions
  G8-W (section wire size) and G8-2 (lazy section in the wasm handle); the new CI steps have never run; no versioned
  fixture with comments (comment-rect path proven only on 10 local real documents).
- **Workspace**: `clippy -D warnings` native + wasm32 for the whole workspace, `clippy-ban`, `float-casts`,
  `grep-bans` all green; suites uo-docx (all features), uo-docx-layout, uo-wasm, uo-fonts green. Pre-existing F2
  float casts at `session/ops/lists.rs:761/1092/1126` are only visible with the `session` feature (the script runs
  without features) — fix and extend `float-casts.sh` next.

Next: queue item 2 (uo-render frontiers D and V).

## Update 2026-11-10 — milestone-1 code done (queue items 2–4)

Same workflow (Opus plan → Sonnet implement → Opus review; all three accepted after fix rounds):

- **uo-render D + V**: the TS painter of decoded pages (`core/crates/uo-wasm/js/paint.ts`, `picture.ts`, mask exports
  in uo-wasm/uo-render; look→spec helpers moved from uo-pdf to `uo_render::paint::picture`) and painter parity
  (`tests/paint_parity.test.mjs` Parts A–C, `browser/paint_pixels.mjs`, `paint_memory.test.mjs`). Record: `uo-render.md`.
- **G4**: `LayoutOptions.cancel` / `LayoutError::Cancelled` in `Budget::charge`, wired to uo-ffi (code 10), the C#
  facade and wasm (`cancelCheck`, import `uo_host.layout_poll`, layout flag bit 9). Records: `uo-docx-layout.md`,
  `uo-bindings.md` G4 row.
- **Viewer switch**: `engine="rust"` on `<universal-document-viewer>` (`write/packages/viewer/src/rust-engine.ts`,
  default `ts` untouched, wasm fetched only on first use as `chunks/uo-write.wasm`), CSP note, CHANGELOG entry. Full
  Chromium parity: **74/74 fixtures, 579 pages, 0 differences** (pixels exact, text layer, find, links, PDF).
- Verified by the orchestrator: workspace clippy native + wasm32, bans, suites uo-docx/layout/render/pdf/wasm/ffi/fonts
  green.

Open before the milestone-1 gate (besides queue item 5):
- Rust path opens slower in the browser (loadDocx median 251 ms vs TS 122 ms on the fixtures) — measure and explain.
- `tools/build-pkg.mjs` ships `uo-write.wasm` without the licence notices of the Rust crates compiled in — must add a
  THIRD-PARTY notice (CLAUDE.md rule) before any publish from rust-core.
- CI lines for D/V/G4/switch not added to `rust-core.yml`; Part A of the painter parity is platform-pinned (win32) and
  would always skip on Linux CI — recapture or relax the pin.
- `write_wasm2js.test.mjs` fails (`f8__7__0 messages: length 51757 vs 53200`), also at the previous base — investigate.
- Painter not run in WebKit/real Safari (only the handle layer).
- ~~Speed of verification~~: done 2026-11-10 (one uo-docx test binary, `tools/check-changed.mjs`; see the operating
  rules below).

## Update 2026-11-10 (later) — milestone-1 gate work (queue item 5, minus what needs the maintainer)

Workflow `rust-port-milestone1-gate` (3 frontiers plan → implement → review, then security review, milestone skeptic
and benchmarks, then a fix chain; every reviewer accepted). Items above, now:

- **Open speed** — root cause: one `WriteEnv` per viewer element (assets re-pushed, every face re-hashed, a discovery
  pass on every document). Now one environment per page and fonts base, superseded passes not decoded, pages
  decoded on demand (`uo_layout_page_heads`), one face copy out of wasm. Chromium, new element per document: Rust
  median 44–47 ms vs TS 62–72 ms. Still slower in total (S1/S2 9.4 s vs TS 8.9/7.6 s): wasm layout in Chromium
  (9.0 s vs 7.1 s), tier-up unproven (G30). Record: `uo-bindings.md` "Open speed".
- **Licences** — `core/tools/licenses/gen-notices.mjs` → `core/licenses/` (checked with `--check`);
  `build-pkg.mjs` `shipEngineWasm` fails without current notices; the NuGet packs them (and its `runtimes/` path no
  longer repeats the RID). `deny.toml` `[licenses]`.
- **CI** — `rust-core.yml` jobs for painter (Linux recapture of Part A), viewer switch (phase 0, parity, memory,
  faults), wasm2js, cargo deny licenses. **Never run** (branch not pushed).
- **wasm2js** — cause: LLVM O3 turns `OptF64`'s sNaN marker into an `f64.const nan:payload`, which wasm2js prints as
  `NaN`. `OptF64` is now an integer (0 = absent, bits + 1; still 8 B) — this settles **G22**; uo-image TIFF
  `UNDEF_BITS` is a plain NaN; `build-wasm2js.mjs` fails on NaN-payload constants. 5/5 on O3 and z.
- **WebKit** — painter Part A in Playwright WebKit (Windows WebKit 26.5); real Safari 15 still open.
- **F2-20** — the stale-stamped captures were recaptured against the frozen root (`d0f45bc5`).
- **Security review** (`core/docs/security.md`, new) — blocker fixed: 56+ nested tables overflowed the native stack
  in layout (kills the .NET server); `uo_docx_to_pdf` now converts on its own 16 MiB thread
  (`hostile_inputs.rs`, `write_hostile.test.mjs`). Untrusted documents open with the C# package limits (256 MiB per
  part, 512 MiB total). A wasm trap kills the instance and the next use loads a fresh module (`viewer_trap.test.mjs`).
  Per-effect native cancellation of raster effects; fuzz target `ffi_docx_to_pdf` (G21 in part).
  `uo_alloc::assert_no_leak_global` for tests whose work runs on another thread.
- **Benchmarks** (`uo-docx-layout.md` "Benchmarks main × rust-core — speed", `tools/bench/speed/run.mjs`): 68
  captures + 5 generated classes up to 500 pages, PDFs byte-identical in TS, wasm and native. Warm totals vs TS:
  native 0.90, wasm 0.94 (open 0.56/0.61, PDF 0.67/0.77, layout 0.96/0.98). 500-page wasm layout 1.40× TS (G30).
- **Skeptic** — checklist in `uo-bindings.md` "Milestone-1 skeptic review (2026-11)".

- **Real Safari** (2026-11-10, `viewer_switch.mjs --mode serve` + BrowserStack, temporary dev tunnel): 0 differences
  TS × Rust on Safari 15.6.1 (Monterey) and iOS 26.5, 19 documents / 82 pages, flat memory; iOS 15.5 (iPhone 13) 16 of 19
  equal, the 3 largest crash the TWO-viewer harness at dpr 3 (each engine alone opens them). Safari 15 refuses wasm even
  with `'wasm-unsafe-eval'` (needs `'unsafe-eval'`). Record: `uo-bindings.md` "Real Safari 15 / iOS".

**Still open for the milestone-1 gate (needs the maintainer):** push `rust-core` and get one green `rust-core.yml`
run; decisions **G23–G31** in `uo-bindings.md` "For the
milestone-1 gate" (G23 layout stack per level, G24 work budget for text effects — 600 glow runs = 61 s, G25 limits
of both trust levels, G26 wasm2js trap detection, G27/G28 page environment rebuild/shrink, G29 M3 baseline, G30
Chromium/500-page wasm layout and server PDF vs C# untimed, G31 G5 caps on the corpus and the deferred bounds); then
the gate itself.

## Work in progress — commit `6471786d` (historical: finished, see the update above)

It compiles (`cargo check --workspace --all-features --tests`) but was **not verified**:

- **D2b F7 (drawing objects, pictures)**: `session/ops/drawing{,/}.rs`, `pictures.rs`, a Node capture
  (`js/capture-d2b-f7-ops.mjs`), `tests/parity_d2b_f7_ops.rs`, `tests/session_d2b_f7.rs`. No Chromium capture, no
  memory/no-panic tests, no `uo-docx-d2b/F7.md`. Known failures: `parity_d2b_f7_ops` ("12080 equal, 0 mismatches,
  12000 not replayed"); clippy `-D warnings` errors in `session/ops/pictures.rs` and `session/ops/drawing.rs`;
  `core/tests/grep-bans.sh` flags `tests/parity_d2b_f7_ops.rs:16` (an `f64` parse).
- **G8 — `Layout::encode_page` (the Write page section)**: `schema/write.uos` extended and regenerated,
  `uo-docx-layout` declares its codec, `section.rs`/`annotations.rs`, `tests/g8.rs`, `tools/lib/facets-g8.mjs`, the wasm
  `UoLayout` section export and `uo-wasm/tests/write_section.test.mjs`. Parity, sizes and the record sections not
  done. Known: clippy error at `uo-docx-layout/src/annotations.rs:127`; the layout `adapter` test reports "840 not
  replayed" (`p3.*` facets) — confirm whether that predates `6471786d`.
- Also seen at HEAD: clippy on wasm32 fails in `uo-fonts/tests/registry_nopanic.rs` (unresolved `uo_fonts::system`
  import, a native-only module used by a test); `cargo fmt --all --check` hits a Windows "file name too long" error
  (run `rustfmt` per file instead).

**First thing to do next:** finish or revert these two frontiers, then make the whole workspace green
(`clippy -D warnings` native + wasm32, `grep-bans.sh`, `float-casts.sh`, all suites).

## Queue (in order; model per role as in SKILL.md)

Milestone 1 (viewer and PDF in Rust):
1. Finish G8 (Opus — touches the layout crate).
2. uo-render **frontier D** (TS shell: decode pages, canvas painter on decoded pages) and **V** (painter parity in
   Chromium) — Sonnet per the uo-render plan.
3. Cancellation inside the layout (G4: `LayoutOptions.cancel`, `LayoutError::Cancelled` in `Budget::charge`).
4. Viewer TS/Rust switch behind `engine="rust"` (`uo-bindings.md` §2 "The TS/Rust switch of the viewer").
5. Gate work: security review + milestone skeptic (Opus), benchmarks (speed), real Safari 15 / iOS on BrowserStack
   (the MCP needs re-authorisation via `/mcp`), recapture the stale-stamped fixtures (F2-20), **first run of the CI
   on GitHub (never run)**, then the maintainer's milestone-1 gate.

Milestone 2 (editor in Rust):
6. D2b: finish F7; F4 remainder (watermark, cover pages, header/footer galleries, `insertDateTime`,
   `insertDocumentInfo`, `insertAlignmentTab`); **F8 clipboard** (Opus for the paste-tree boundary); **F10** the layout
   family (fields, TOC, captions, manual hyphenation, galleries — needs F8 of the layout, done).
7. Incremental relayout (layout plan row "(M2)", LA7 levels 1–2; Opus).
8. Editor bindings: the session in wasm (`UoWriteSession`, session codec G15) and the D3 C ABI + C# facade.
9. The editor switch: `universal-write.ts` reduced to the shell over the wasm session (the largest unknown).
10. Milestone-2 gate, `CHANGELOG.md` (API breaks: `uo-bindings.md` §4), docs, publish 0.10.

Later: Sheet (calc engine on Opus, `sheet-core`), Slides (`slides-core`), charts, the converter — 0% today.

## Decisions waiting for the maintainer

- **D2a** (record "D2a — session (architecture)", "For the milestone-2 gate"): (1) order-sensitive signatures in
  the session (D2a-U6) vs a per-run order tag in `write::sig`; (2) D2a-C2 — a third `LayoutOracle` method answering
  which header/footer part a page shows, or the shell passes it (F4 implemented the shell-argument variant,
  isolated); (3) notes with tables: TS flattened list vs story editing (F5-D1 kept story editing; no corpus note has
  a table).
- **Bindings G1–G22** (`uo-bindings.md` "For the milestone-1 gate"): notably G4 cancellation, G5 limits for untrusted
  documents, G7 size budget, G8 page section, G14 NuGet RID packing, **G22 `OptF64`'s signalling-NaN "absent" marker**
  (breaks any JS/wasm2js build) — **done 2026-11-10** as an integer encoding (approve), and the new **G23–G31** from
  the gate work.
- **Size** (`size-optimization.md`): D-1 six NaN-capable layout sorts on the JS core; D-2 `opt-level = "s"` for
  uo-docx in the wasm profile (−68 KB gzip, open +14%) or `"z"` (−156 KB, open +60%). Viewer today: 3.96 MB raw /
  1.405 MB gzip (TS start: 0.30 MB gzip).
- **Layout**: the peak of one layout call is ≈ 2× the retained layout (174 MiB at 500 pages), over the plan's target
  as written; M3 retained 45–99 B/char (target 48 on the 500-page document: met).
- **Published product (outside the port, `main` is frozen)**: D3 review S1 (a `javascript:` hyperlink survives the
  C# sanitizer) and S6 (the C# importer's recursion overflows the stack on hostile documents — kills a server
  process); the Slides viewer (`slides/packages/viewer/src/universal-slides-viewer.ts:1161`) and Sheet
  (`sheet/packages/sheet/src/universal-sheet.ts:1559`) open hyperlink targets without a scheme filter (Write filters).
  All in `docs/security.md`.
- CSP: confirmed on a real Safari 15.6.1 — `'wasm-unsafe-eval'` does not let it compile wasm, only `'unsafe-eval'`
  does; strict-CSP hosts on Safari 15 need the TS engine or the wasm2js build (decide what the viewer does there).

## Operating rules learned in this session (follow them)

- **Faster verification (2026-11-10)**: uo-docx's 160 integration-test files are ONE test binary (`tests/all.rs`,
  `autotests = false`; each file is a module, the support modules are declared once there and imported with
  `use crate::<name>;`). A brief's `--test <file>` for uo-docx is now `--test all <file>::` (older records still show
  the old form). New test file = add `mod <file>;` to `tests/all.rs`. Local lint gate: `node tools/check-changed.mjs
  [base]` (clippy native + wasm32 and the bans on the changed crates and their dependents); the whole-workspace clippy
  is for CI and milestone checks.
- **Machine load**: at most 2–3 agents at once; every brief demands `cargo -j 4`, tests with `--test-threads` ≤ 4,
  bounded no-panic/soak tests (seconds; big runs behind an env var), own `CARGO_TARGET_DIR` under `core/target/<name>`,
  and "only your own test targets". **Only the orchestrator runs the full suite**, once per commit
  (`cargo test -j 4 --release -p uo-docx --no-fail-fast --features canon,editing-api,doc,odf,export,compare,session -- --test-threads=3`).
  With 5–7 agents the machine froze (a `nopanic_d2b_f3` test).
- **Junctions**: agents have built sandboxes inside `core/target/` with junctions to `write/`, `shared/`…; briefs now
  forbid sandboxes, repo copies, junctions and symlinks. Before deleting anything under `target/`, check for links
  with Node (`fs.lstatSync(p).isSymbolicLink()`, two levels deep) — not `cmd dir /AL` (it prints "File Not Found",
  which counts as a line). Remove a junction with `fs.unlinkSync`. Never `rm -rf` through `node_modules`.
- **Commits**: one per verified wave on `rust-core`, no push; never `git stash` (the stack is shared with other
  worktrees). Shared files edited by two running agents (`uo-docx.md`, `session/mod.rs`, `engine/*.rs`, `lib.rs`) are
  committed together when both are done. The pre-commit hook checks fonts by content — never bypass it (a fixture
  with a 64-byte zero "font" was allow-listed in `tools/fonts/allowed-fonts.txt` with its explanation).
- **Verify agent reports** before committing: rerun their suites, clippy native + wasm32, `grep-bans.sh`; agents have
  left lint errors, `f64::clamp` (banned by `clippy.toml`), stray files (a 0-byte file named with a private-use
  character) and an empty capture script.
- **Tools**: edit `.rs`/`.ts` with the Edit tool (CRLF files; `sed -i` converts them); the Bash tool halves double
  backslashes (write scripts with regexes through the Write tool); Playwright 1.61 needs `UO_CHROMIUM` pointing at the
  installed `chromium-1248` (its own revision is not downloaded); binaryen 132 (`wasm-opt`, `wasm2js`) is installed
  in `core/target/tools/binaryen` (npm, outside every `package.json`).
- Size tools: `core/tools/size/` (`wasm-size.mjs`, `wasm-share.mjs` — gzip cost of a function set, `wasm-ab.mjs` —
  interleaved speed A/B with an output digest; separate-process timings are useless on this machine).
