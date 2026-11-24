// TS baseline of the editor operations (frontier C5b): the frozen `<universal-sheet>` element on the fake DOM (the harness of
// T10a), the same scenarios as `examples/edit_bench.rs`: one-cell entry latency (an isolated cell, the head of a chain), a
// formula entry, paste of a 100 x 100 internal copy, Fill Down over 10,000 / 100,000 cells, Replace All, undo/redo of each.
//   node --expose-gc --max-old-space-size=8192 core/crates/uo-xlsx/js/bench-edit.mjs sheet/samples/giant-1m.xlsx core/target/sheet-cap/bench/calc-heavy-20000.xlsx
process.env.TZ = 'UTC';
const { readFileSync } = await import('node:fs');
const { pathToFileURL } = await import('node:url');
const C = await import('./lib/common.mjs');
const { buildHarness } = await import('./harness/build-harness.mjs');

const h = buildHarness(C.tsRoot(C.args()), `${C.coreDir}/target/sheet-cap/harness`);
const dom = (await import(pathToFileURL(h.dom).href)).installFakeDom();
dom.customElements.define('u-ripple', class extends globalThis.HTMLElement {});
const M = await import(pathToFileURL(h.sheet).href);
globalThis.DOMMatrix = class { constructor(m) { [this.a, this.b, this.c, this.d, this.e, this.f] = m; } };
const quiet = (f) => { const w = console.warn, e = console.error; console.warn = console.error = () => {}; try { return f(); } finally { console.warn = w; console.error = e; } };
const ms = (f) => { const t = performance.now(); const r = f(); return [performance.now() - t, r]; };
const pct = (a, p) => { const s = [...a].sort((x, y) => x - y); return s[Math.min(s.length - 1, Math.floor(p * s.length))]; };
const N = Number(process.env.UO_EDITS ?? 20);

for (const f of process.argv.slice(2).filter((a) => a.endsWith('.xlsx'))) {
  const el = new M.UniversalSheetElement();
  el.setAttribute('formula-locale', 'pt-BR');
  dom.document.body.appendChild(el);
  el.paint = () => {};
  const [open] = ms(() => quiet(() => el.loadXlsx(new Uint8Array(readFileSync(f)))));
  quiet(() => el.recalculate());
  const lat = (ref, texts) => texts.map((t) => { el.selectRange(ref); return ms(() => quiet(() => el.setActiveCellText(t)))[0]; });
  lat('A2', ['7', '8']); lat('Z1', ['1', '2']);
  const chain = lat('A2', Array.from({ length: N }, (_, i) => String(i + 3)));
  const iso = lat('Z1', Array.from({ length: N }, (_, i) => String(i + 3)));
  const formula = lat('Z2', Array.from({ length: N }, (_, i) => `=Z1*${i + 2}`));
  const out = { file: f.split(/[\\/]/).pop(), openMs: +open.toFixed(1) };
  out.chain = { p50: +pct(chain, 0.5).toFixed(3), p95: +pct(chain, 0.95).toFixed(3) };
  out.isolated = { p50: +pct(iso, 0.5).toFixed(3), p95: +pct(iso, 0.95).toFixed(3) };
  out.formula = { p50: +pct(formula, 0.5).toFixed(3), p95: +pct(formula, 0.95).toFixed(3) };
  const op = (name, f) => { const [t] = ms(() => quiet(f)); const [u] = ms(() => quiet(() => el.undo())); const [r] = ms(() => quiet(() => el.redo())); out[name] = { ms: +t.toFixed(1), undo: +u.toFixed(1), redo: +r.toFixed(1) }; };
  const heavy = f.includes('calc-heavy');
  op(heavy ? 'paste30x30' : 'paste100x100', () => { el.selectRange(heavy ? 'A1:AD30' : 'A1:CV100'); el.copy(); el.selectRange('DA1'); el.paste(); });
  if (heavy) {
    op('fillB1:B21', () => { el.selectRange('B1:B21'); el.fill('down'); });
    op('fillAA1:AA1000', () => { el.selectRange('AA1'); el.setActiveCellText('5'); el.selectRange('AA1:AA1000'); el.fill('down'); });
  } else op('fill10000', () => { el.selectRange('B1:B10000'); el.fill('down'); });
  if (process.env.UO_BIG_OPS) op('fill100k', () => { el.selectRange('C1:C100000'); el.fill('down'); });
  op('replaceAll', () => { el.replaceAll('§', '8'); });
  console.log(JSON.stringify(out));
  el.remove();
}
