// C5b: edit latency of `uo-sheet-editor.wasm` in Node (T13 row "edit latency, giant-1m: <= 16 ms p95 wasm"): one cell entry
// (+ the dependents' recalc) and, separately, the entry followed by the 1080p viewport list of the edited sheet (the
// "list ready" of the row: the viewer is rebuilt and the sheet re-activated after a change, open item of the routing).
//   node --max-old-space-size=8192 crates/uo-wasm/tests/sheet_edit_bench.mjs [<uo-sheet-editor.wasm>] <xlsx>...
import { readFileSync } from 'node:fs';
import { bindSheetEditor } from '../js/sheet-editor.mjs';

const args = process.argv.slice(2);
const wasmPath = args[0]?.endsWith('.wasm') ? args.shift() : new URL('../../../target/pkg-wasm/uo-sheet-editor.wasm', import.meta.url);
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const ue = bindSheetEditor(instance.exports);
const N = Number(process.env.UO_EDITS ?? 20);
const pct = (a, p) => { const s = [...a].sort((x, y) => x - y); return s[Math.min(s.length - 1, Math.floor(p * s.length))]; };
const measurer = { advance: () => 7, width: (_f, s) => 7 * s.length, box: () => [9, 3] };
for (const f of args) {
  let t = performance.now();
  const ed = ue.open(new Uint8Array(readFileSync(f)), { locale: 'pt-BR' });
  const open = performance.now() - t;
  const frame = () => { try { return ed.viewport({ sheet: 0, width: 1920, height: 1080, zoom: 1, dpr: 1, scrollX: 0, scrollY: 0, selection: [2, 2, 2, 2, 2, 2] }, measurer); } catch (e) { return e; } };
  t = performance.now(); frame(); const first = performance.now() - t;
  const lat = (ref, texts, withFrame) => texts.map((x) => { ed.cmd('select', ref); const a = performance.now(); ed.cmd('enter', x); if (withFrame) frame(); return performance.now() - a; });
  lat('A2', ['7', '8'], false); lat('Z1', ['1', '2'], true);
  const iso = lat('Z1', Array.from({ length: N }, (_, i) => String(i + 3)), false);
  const head = lat('A2', Array.from({ length: N }, (_, i) => String(i + 3)), false);
  const isoF = lat('Z1', Array.from({ length: N }, (_, i) => String(i + 3)), true);
  const r = (a) => `${pct(a, 0.5).toFixed(2)}/${pct(a, 0.95).toFixed(2)}`;
  console.log(`${f}: open ${open.toFixed(0)} ms, first list ${first.toFixed(1)} ms; entry p50/p95 ms: isolated ${r(iso)}, A2 ${r(head)}; entry + 1080p list ${r(isoF)}; wasm memory ${(instance.exports.memory.buffer.byteLength / 1048576).toFixed(0)} MiB`);
  ed.free();
}
