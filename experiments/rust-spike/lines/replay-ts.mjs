// Réplica do spike 2 no motor TS: reconstrói os parágrafos capturados (com as faces reais do
// provedor do sistema), repete as chamadas new / next / reset no LineBreaker de verdade e
// confere cada linha com a captura. Mede CPU e memória da quebra (só a quebra).
//   node --expose-gc replay-ts.mjs [iterações]
import { readFileSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import v8 from 'node:v8';

const here = dirname(fileURLToPath(import.meta.url));
// UO_TS_ROOT: checkout whose TS engine (built dist) is measured — main, the frozen baseline
const root = process.env.UO_TS_ROOT ? resolve(process.env.UO_TS_ROOT) : resolve(here, '../../..');
const imp = (p) => import(pathToFileURL(join(root, p)).href);
const { LineBreaker } = await imp('write/packages/docx-layout/dist/lines.js');
const { createNodeFontProvider } = await imp('write/tools/corpus-lab/node-fonts.mjs');
const ITERS = Number(process.argv[2] ?? 20);

const data = JSON.parse(readFileSync(join(here, '../data/lines.json'), 'utf8'));
const provider = await createNodeFontProvider({ missing: [] });
const faces = data.faces.map((f) => provider.face(f.family, f.bold, f.italic));
const missing = data.faces.filter((f, i) => !faces[i]).map((f) => f.family);
if (missing.length) throw new Error(`faces ausentes: ${missing.join(', ')}`);

const glyph = (o) => ({
  kind: o.k, ch: o.ch, cp: o.cp, adv: o.adv, asc: o.asc, desc: o.desc, offset: o.off, runIdx: 0, breakAfter: o.br, sizePt: o.sz,
  face: faces[o.f],
  style: { underline: o.ul ? { val: 'single' } : undefined, rpr: {} },
  ...(o.nAsc !== undefined ? { nAsc: o.nAsc } : {}), ...(o.nDesc !== undefined ? { nDesc: o.nDesc } : {}),
  ...(o.glue ? { glue: true } : {}), ...(o.clear ? { clear: true } : {}), ...(o.hr !== undefined ? { hr: o.hr } : {}),
  ...(o.ptab ? { ptab: { alignment: o.ptab.a, ...(o.ptab.l ? { leader: o.ptab.l } : {}) } } : {}),
  ...(o.fe !== undefined ? { feShrink: o.fe } : {}), ...(o.fixed ? { fixed: true } : {}), ...(o.hang ? { hang: true } : {}), ...(o.hyph ? { hyph: true } : {}),
});
const ctxs = new Map();
const ctxOf = (s) => {
  const k = JSON.stringify(s);
  let c = ctxs.get(k);
  if (!c) {
    c = { fmt: { settings: { defaultTabStop: s.defaultTabStop, compatibilityMode: s.compatibilityMode, consecutiveHyphenLimit: s.consecutiveHyphenLimit,
      ...(s.hyphenationZone !== null ? { hyphenationZone: s.hyphenationZone } : {}),
      compat: { ...(s.doNotUseIndentAsNumberingTabStop ? { doNotUseIndentAsNumberingTabStop: true } : {}), ...(s.splitPgBreakAndParaMark ? { splitPgBreakAndParaMark: true } : {}) } } },
      opts: { ...data.opts } };
    ctxs.set(k, c);
  }
  return c;
};
const paras = data.paras.map((p) => ({
  ctx: ctxOf(p.settings), grid: p.grid ?? undefined,
  p: {
    glyphs: p.glyphs.map(glyph), tabs: p.tabs.map((t) => ({ val: t.val, pos: t.pos, ...(t.leader ? { leader: t.leader } : {}) })),
    firstLine: p.firstLine, indLeft: p.indLeft, indRight: p.indRight, jc: p.jc, lineRule: p.lineRule, line: p.line,
    marker: p.marker ? { jc: p.marker.jc, width: p.marker.width, suff: p.marker.suff, glyphs: p.marker.glyphs.map(glyph) } : undefined,
    markFace: faces[p.markFace],
    markStyle: { sizePt: p.markStyle.sizePt, shift: p.markStyle.shift, rpr: { ...(p.markStyle.vertAlign ? { vertAlign: p.markStyle.vertAlign } : {}), ...(p.markStyle.sz !== null ? { sz: p.markStyle.sz } : {}), ...(p.markStyle.textShadow ? { textShadow: p.markStyle.textShadow } : {}) } },
  },
}));

function replay() {
  const brs = [], out = [];
  for (const e of data.events) {
    if (e[0] === 'new') { const q = paras[e[1]]; brs[e[1]] = new LineBreaker(q.ctx, q.p, q.grid); }
    else if (e[0] === 'next') out.push(brs[e[1]].next(e[2]));
    else brs[e[1]].reset(e[2]);
  }
  return out;
}

// paridade: cada linha igual à captura (glifo do quadro por índice; -1 = hífen automático)
const ref = (q, p) => { const i = p.glyphs.indexOf(q.g); if (i >= 0) return i; const m = p.marker ? p.marker.glyphs.indexOf(q.g) : -1; return m >= 0 ? -2 - m : -1; };
function parity(out) {
  let bad = 0;
  const lineParas = data.events.filter((e) => e[0] === 'next').map((e) => paras[e[1]].p);
  out.forEach((l, n) => {
    const c = data.lines[n], p = lineParas[n];
    const same = l.start === c.start && l.end === c.end && l.width === c.width && l.xStart === c.xStart && l.asc === c.asc && l.desc === c.desc
      && l.height === c.height && l.baseline === c.baseline && l.last === c.last && (l.breakAfter ?? null) === c.breakAfter && !!l.clear === c.clear
      && (l.gapTab ?? null) === c.gapTab && !!l.spanned === c.spanned && l.placed.length === c.placed.length
      && l.placed.every((q, k) => ref(q, p) === c.placed[k][0] && q.x === c.placed[k][1] && q.w === c.placed[k][2] && (q.leader ?? null) === c.placed[k][3]);
    if (!same) bad++;
  });
  return bad;
}

const gc = () => { global.gc(); global.gc(); };
replay(); replay();
const bad = parity(replay());

// CPU: tempo de parede e de CPU por iteração
const times = [];
const cpu0 = process.cpuUsage();
for (let i = 0; i < ITERS; i++) { const t = performance.now(); replay(); times.push(performance.now() - t); }
const cpu = process.cpuUsage(cpu0);
times.sort((a, b) => a - b);

// memória alocada numa iteração: soma do que cada GC recolheu + o que sobrou no fim
gc();
const prof = new v8.GCProfiler();
const h0 = v8.getHeapStatistics().used_heap_size;
prof.start();
let kept = replay();
const h1 = v8.getHeapStatistics().used_heap_size;
const st = prof.stop().statistics;
let allocated = 0, prev = h0;
for (const s of st) { allocated += s.beforeGC.heapStatistics.usedHeapSize - prev; prev = s.afterGC.heapStatistics.usedHeapSize; }
allocated += h1 - prev;
// memória retida pelas linhas produzidas (o resultado da quebra que o layout guarda)
gc();
const withLines = v8.getHeapStatistics().used_heap_size;
const keptCount = kept.length;
kept = null;
gc();
const without = v8.getHeapStatistics().used_heap_size;

const glyphs = data.paras.reduce((a, p) => a + p.glyphs.length, 0);
const r = (v) => Math.round(v * 100) / 100;
console.log(JSON.stringify({
  impl: 'ts-node', lines: keptCount, glyphs, mismatches: bad,
  bestMs: r(times[0]), medianMs: r(times[Math.floor(times.length / 2)]), cpuMsPerIter: r((cpu.user + cpu.system) / 1000 / ITERS),
  allocatedMB: r(allocated / 1048576), retainedMB: r((withLines - without) / 1048576),
  peakRssMB: r(process.resourceUsage().maxRSS / 1024), heapUsedMB: r(without / 1048576),
}));
