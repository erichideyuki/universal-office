// The Sheet viewer module in Node (frontier C4b, record core/docs/modules/uo-xlsx.md "C4b — as built"): open -> first frame and the
// 1080p emission time per frame of `uo-sheet.wasm` (or `uo-sheet-calc.wasm`, which applies the open-recalc at the open), the fake
// measurer of the harness (6 px per unit), medians.
//
//   node core/crates/uo-xlsx/js/bench-wasm.mjs <uo-sheet.wasm> <book.xlsx>...
import { readFileSync } from 'node:fs';
import { bindSheet } from '../../uo-wasm/js/sheet.mjs';

const [wasmPath, ...books] = process.argv.slice(2);
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const uo = bindSheet(instance.exports);
const m = { advance: (_f, cp) => (cp > 0xffff ? 12 : 6), width: (_f, t) => 6 * t.length, box: () => [9, 3] };
const answer = (reqs) => reqs.flatMap((r) => (r.kind === 'metrics' ? [6 * r.text.length, 0, 6 * r.text.length, 8, 2, 9, 3] : r.kind === 'kernAt1000' ? [500, 500] : r.kind === 'imageSize' ? [40, 30] : r.kind === 'metaText' ? [10, 9, 3] : r.kind === 'metaDecode' ? [NaN, NaN] : [r.kind === 'charAt1000' ? 500 : 6 * (r.text?.length ?? 1)]));
const median = (v) => v.slice().sort((a, b) => a - b)[v.length >> 1];
console.log(`module ${wasmPath}: calc ${uo.hasCalc()}`);
for (const f of books) {
  const bytes = new Uint8Array(readFileSync(f));
  const t0 = performance.now();
  const book = uo.open(bytes);
  const tOpen = performance.now() - t0;
  const si = Math.max(0, book.sheets().findIndex((s) => s.active));
  book.activate(si, m);
  const q = (k, zoom) => ({ sheet: si, width: 1920, height: 1080, dpr: 1, scrollX: (k % 40) * 11.5, scrollY: (k % 90) * 37, zoom, selection: [2, 2, 2, 2, 4, 6] });
  const first = book.viewport(q(0, 1), m, answer);
  const tFirst = performance.now() - t0;
  for (let k = 0; k < 20; k++) book.viewport(q(k, 1), m, answer);
  const ts = [];
  let bytesOut = 0;
  for (let k = 0; k < 200; k++) {
    const t = performance.now();
    const b = book.viewport(q(k, 1), m, answer);
    ts.push(performance.now() - t);
    bytesOut = b.length;
  }
  console.log(`${f}: open ${tOpen.toFixed(1)} ms (plan ${book.recalcPlan()}), open -> first frame ${tFirst.toFixed(1)} ms (${first.length} B); 1080p z1 frame ${median(ts).toFixed(3)} ms median (min ${Math.min(...ts).toFixed(3)}), message ${bytesOut} B; wasm memory ${(instance.exports.memory.buffer.byteLength / 1048576).toFixed(1)} MiB`);
  book.free();
}
