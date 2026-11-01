# Benchmarks 2026-11-01

main d0f45bc5 × rust-core 9ac45738 (with uncommitted changes)

### uo-opc: open + read + parse of the oracle (2742 docx, 17798 parts, 729760 elements; Write preset with positions; memory per document)

|  | TS (V8) | Rust native | Rust wasm |
| --- | --- | --- | --- |
| best (ms) | 584.5 | 259.6 | 279.6 |
| allocated (MB) | — | 394.2 | 361.3 |
| peak (MB; TS: max RSS growth) | 29.3 | 0.8 | 0.8 |
| kept, outputs alive (MB) | 0.3 | 0.0 | 0.0 |
| wasm memory growth (MB) | — | — | 1.0 |
| same totals as TS | — | true | true |

### uo-opc: open + read + parse of giant-1m.xlsx (2010047 elements; Sheet presets)

|  | TS (V8) | Rust native | Rust wasm |
| --- | --- | --- | --- |
| best (ms) | 742.4 | 165.2 | 185.4 |
| allocated (MB) | — | 355.4 | 355.4 |
| peak (MB; TS: max RSS growth) | 826.6 | 146.1 | 146.1 |
| kept, outputs alive (MB) | 31.9 | 34.6 | 34.6 |
| wasm memory growth (MB) | — | — | 203.9 |
| same totals as TS | — | true | true |

### uo-opc: inflate of giant-1m.xlsx sheet1.xml (31.7 MB out)

|  | TS (V8) | Rust native | Rust wasm |
| --- | --- | --- | --- |
| best (ms) | 60 | 38.5 | 42.7 |
| allocated (MB) | — | 92.8 | 92.8 |
| peak (MB; TS: max RSS growth) | 30.6 | 34.2 | 34.2 |
| kept, outputs alive (MB) | 31.8 | 0.0 | 0.0 |
| wasm memory growth (MB) | — | — | 59.9 |
| MB/s | 529 | 824 | 743 |

### uo-opc: edit + save of sw-16-8.docx (TS deflates every part, Rust copies the untouched ones raw)

|  | TS (V8) | Rust native | Rust wasm |
| --- | --- | --- | --- |
| best (ms) | 2.5 | 0.8 | 1.2 |
| allocated (MB) | — | 2.1 | 2.2 |
| peak (MB; TS: max RSS growth) | 5.0 | 0.7 | 0.8 |
| kept, outputs alive (MB) | 0.3 | 0.2 | 0.2 |
| wasm memory growth (MB) | — | — | 0.9 |
| output (B) | 5362 | 5358 | 5358 |

### uo-opc: edit + save of giant-1m.xlsx with sheet1.xml edited (deflated again)

|  | TS (V8) | Rust native | Rust wasm |
| --- | --- | --- | --- |
| best (ms) | 575.5 | 818.5 | 1007.9 |
| allocated (MB) | — | 181.5 | 181.7 |
| peak (MB; TS: max RSS growth) | 89.4 | 95.2 | 95.2 |
| kept, outputs alive (MB) | 3.1 | 31.7 | 31.7 |
| wasm memory growth (MB) | — | — | 126.2 |
| output (B) | 2848422 | 2856998 | 2856998 |

### uo-opc: wasm module (feature opc-bench)

|  | bytes |
| --- | --- |
| uo_wasm.wasm | 270853 |
