// A carga do spike no SfntFont do C# compilado para WebAssembly (o build do wasm-poc), no Node.
//   node bench-cs-wasm.mjs ../wasm-poc/out/lean-aot
import { readFileSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const PARSE_ITERS = 10, MEASURE_ITERS = 20, SIZE_PT = 11;
const here = dirname(fileURLToPath(import.meta.url));
const build = resolve(here, process.argv[2] ?? '../wasm-poc/out/lean-aot');
const dir = join(here, 'data');
const names = readFileSync(join(dir, 'fonts.txt'), 'utf8').split('\n').filter(Boolean);

const t0 = performance.now();
const { dotnet } = await import(pathToFileURL(join(build, 'wwwroot/_framework/dotnet.js')).href);
const runtime = await dotnet.create();
const bench = (await runtime.getAssemblyExports('WasmPoc.dll')).WasmPoc.FontBench;
const startMs = performance.now() - t0;
for (const n of names) bench.AddFont(new Uint8Array(readFileSync(join(dir, 'fonts', n))));
const text = readFileSync(join(dir, 'text.txt'), 'utf8');
bench.SetText(text);

bench.ParseAll(1);
bench.MeasureAll(SIZE_PT, 1);
const parseMs = bench.ParseAll(PARSE_ITERS);
const measureMs = bench.MeasureAll(SIZE_PT, MEASURE_ITERS);
const total = bench.Total(SIZE_PT);
console.log(JSON.stringify({ impl: `csharp-wasm-${build.split(/[\\/]/).pop()}`, startMs: +startMs.toFixed(1), parseMs: +parseMs.toFixed(2), measureMs: +measureMs.toFixed(2), glyphs: [...text].length * names.length, total: total.toFixed(6) }));
process.exit(0);
