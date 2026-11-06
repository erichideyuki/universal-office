// Speed and memory of one engine of the viewer module in Node: layout, all pages (render and Write sections) and the
// PDF per fixture, median of --runs (default 5) after a warm-up, plus the load time and the process high-water mark
// (maxRSS, so run one engine per process). Record: core/docs/modules/uo-bindings.md.
//
//   node core/crates/uo-wasm/tools/bench-wasm2js.mjs --engine wasm --wasm <uo_wasm.wasm>
//   node core/crates/uo-wasm/tools/bench-wasm2js.mjs --engine js --js <uo_wasm.js>
//   (browsers: tests/browser/wasm2js_run.mjs --mode speed)
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { loadWrite } from '../js/write.mjs';
import { loadAssets, timeFixture } from '../tests/wasm2js_common.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../..');
const args = process.argv.slice(2);
const opt = (n, d) => { const i = args.indexOf(n); return i < 0 ? d : args[i + 1]; };
const engine = opt('--engine', 'wasm'), runs = Number(opt('--runs', '5'));
const keys = ['f8__7__0', 'f8__7__10', 'f8__7__9', 'f8__7__7'];
const assets = await loadAssets(async (p) => new Uint8Array(readFileSync(join(repo, p))), keys);
const t0 = performance.now();
const lib = await loadWrite(engine === 'wasm' ? { engine, wasm: new Uint8Array(readFileSync(opt('--wasm'))) } : { engine, js: pathToFileURL(resolve(opt('--js'))).href });
const loadMs = performance.now() - t0;
const rows = {};
for (const k of keys) rows[k] = timeFixture(lib.uo, assets, k, runs);
const mem = process.memoryUsage();
console.log(JSON.stringify({ engine, loadMs: Math.round(loadMs), rows, linearMemory: lib.exports.memory.buffer.byteLength, maxRssMB: Math.round(process.resourceUsage().maxRSS / 1024), rssMB: Math.round(mem.rss / 1048576), heapUsedMB: Math.round(mem.heapUsed / 1048576) }));
