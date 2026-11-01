# Benchmarks 2026-11-01

main d0f45bc5 × rust-core 122ead8e (with uncommitted changes)

### uo-opc: open + read + parse of the oracle (2742 docx, 17798 parts, 729760 elements; Write preset with positions; memory per document)

|  | TS (V8) | Rust native | Rust wasm |
| --- | --- | --- | --- |
| best (ms) | 559.4 | 226.9 | 244.6 |
| allocated (MB) | — | 252.8 | 219.7 |
| peak (MB; TS: max RSS growth) | 28.7 | 0.5 | 0.5 |
| kept, outputs alive (MB) | 0.3 | 0.0 | 0.0 |
| wasm memory growth (MB) | — | — | 0.6 |
| same totals as TS | — | true | true |

### uo-opc: open + read + parse of giant-1m.xlsx (2010047 elements; Sheet presets)

|  | TS (V8) | Rust native | Rust wasm |
| --- | --- | --- | --- |
| best (ms) | 699.1 | 149.6 | 172.9 |
| allocated (MB) | — | 197.2 | 197.2 |
| peak (MB; TS: max RSS growth) | 834.9 | 103.1 | 103.1 |
| kept, outputs alive (MB) | 31.9 | 2.8 | 2.8 |
| wasm memory growth (MB) | — | — | 116.7 |
| same totals as TS | — | true | true |

### uo-opc: inflate of giant-1m.xlsx sheet1.xml (31.7 MB out)

|  | TS (V8) | Rust native | Rust wasm |
| --- | --- | --- | --- |
| best (ms) | 59.4 | 27.6 | 28 |
| allocated (MB) | — | 57.4 | 57.4 |
| peak (MB; TS: max RSS growth) | 30.3 | 31.8 | 31.8 |
| kept, outputs alive (MB) | 31.8 | 0.0 | 0.0 |
| wasm memory growth (MB) | — | — | 57.4 |
| MB/s | 534 | 1149 | 1133 |

### uo-opc: edit + save of sw-16-8.docx (TS deflates every part, Rust copies the untouched ones raw)

|  | TS (V8) | Rust native | Rust wasm |
| --- | --- | --- | --- |
| best (ms) | 2.4 | 0.6 | 0.9 |
| allocated (MB) | — | 1.5 | 1.6 |
| peak (MB; TS: max RSS growth) | 5.2 | 0.7 | 0.8 |
| kept, outputs alive (MB) | 0.3 | 0.2 | 0.2 |
| wasm memory growth (MB) | — | — | 0.9 |
| output (B) | 5362 | 5358 | 5358 |

### uo-opc: edit + save of giant-1m.xlsx with sheet1.xml edited (deflated again)

|  | TS (V8) | Rust native | Rust wasm |
| --- | --- | --- | --- |
| best (ms) | 554.4 | 792.3 | 957.1 |
| allocated (MB) | — | 131.8 | 131.9 |
| peak (MB; TS: max RSS growth) | 88.3 | 95.2 | 95.2 |
| kept, outputs alive (MB) | 3.1 | 31.7 | 31.7 |
| wasm memory growth (MB) | — | — | 123.7 |
| output (B) | 2848422 | 2856998 | 2856998 |

### uo-opc: wasm module (feature opc-bench)

|  | bytes |
| --- | --- |
| uo_wasm.wasm | 284701 |
