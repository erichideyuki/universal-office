# <crate> — <module>

Record of the rust-port workflow for one module. Copy to `<crate>-<module>.md`.

## Scope

- TS covered (files/functions): …
- C# covered (behaviour the TS does not have): …
- Left for other modules: …

## Plan (gate 1 — approved YYYY-MM-DD)

- Crate and reuse by Sheet/Slides: …
- Data model and ownership (memory scopes: per document / layout pass / operation): …
- Public API (and what changed from 0.9 for the better): …
- Crate vs own port: …
- Parity: capture points, corpus, compared fields: …
- Memory: operations under M1, soak scenario (iterations, tolerance), budgets, caches and
  caps, boundary handles: …
- Security: inputs, limits, fuzz targets: …

## Analysis skeptic

| # | Finding | Blocking | Decision |
|---|---|---|---|

## Verification (gate 2 — YYYY-MM-DD)

### Parity

Corpus: … documents / … items. Mismatches: 0. Differential: … generated documents.

### Memory (measured)

| Check | Result | Numbers |
|---|---|---|
| M1 no leak per operation | | |
| M2 soak (native) | | iterations, min/max live |
| M2 soak (wasm) | | memory.buffer high-water |
| M3 budgets | | allocated / peak per operation |
| M4 boundaries | | |
| M5 cache caps | | |
| M6 Miri / LeakSanitizer | | |

### Security

Findings, fuzz targets (time, executions, crashes).

## Evolution

| Change | Keeps results? | Before | After | Decision |
|---|---|---|---|---|

## Final review and benchmarks (gate 3 — YYYY-MM-DD)

Implementation skeptic findings and decisions. `tools/bench` table (main × rust-core).
