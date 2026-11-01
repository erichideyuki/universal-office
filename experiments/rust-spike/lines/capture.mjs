// Captura do spike 2: roda o motor TS (dist) sobre os casos do oráculo do Word e grava tudo o
// que a quebra de linha (LineBreaker, lines.ts) recebeu e devolveu — os parágrafos preparados,
// a sequência de chamadas (new / next(frame) / reset(mark)) e cada linha produzida. TS, C# e
// Rust repetem exatamente essas chamadas e as linhas têm de bater.
//
// As fontes são as instaladas (como o gen-parity-layout); documentos com fontes embutidas
// ficam de fora (as réplicas pegam as faces do provedor do sistema). O que a quebra pede às
// fontes vai pré-calculado por glifo (largura do hífen, métricas da face) para o Rust.
//   node capture.mjs [--max N]   →   ../data/lines.json
import { readFileSync, writeFileSync, readdirSync, mkdirSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
// UO_TS_ROOT: checkout whose TS engine (built dist) is measured — main, the frozen baseline
const root = process.env.UO_TS_ROOT ? resolve(process.env.UO_TS_ROOT) : resolve(here, '../../..');
const imp = (p) => import(pathToFileURL(join(root, p)).href);
const core = await imp('write/packages/docx-core/dist/index.js');
const eng = await imp('write/packages/docx-layout/dist/index.js');
const { LineBreaker } = await imp('write/packages/docx-layout/dist/lines.js');
const { advanceTwips, shadowExtent } = await imp('write/packages/docx-layout/dist/para.js');
const { faceMetrics } = await imp('write/packages/docx-layout/dist/fonts.js');
const { createNodeFontProvider } = await imp('write/tools/corpus-lab/node-fonts.mjs');

const args = process.argv.slice(2);
const max = Number(args.includes('--max') ? args[args.indexOf('--max') + 1] : 1e9);
// orçamento de glifos (o JSON tem ~215 bytes por glifo): os casos entram em rodízio pelos
// grupos do oráculo, um de cada grupo por rodada, até o orçamento
const budget = Number(args.includes('--glyphs') ? args[args.indexOf('--glyphs') + 1] : 300000);
const provider = await createNodeFontProvider({ missing: [] });
await eng.loadHyphenation(eng.HYPHENATION_PATTERN_IDS);
const opts = { advanceEm: 'nominal', hyphenationLanguages: ['pt-BR', 'en', 'es'] };

const oracle = join(root, 'write/packages/docx-layout/test/oracle');
const groups = [];
for (const g of readdirSync(oracle).sort()) {
  let names = [];
  try { names = readdirSync(join(oracle, g)).filter((f) => f.endsWith('.docx')).sort(); } catch { continue; }
  if (names.length) groups.push(names.map((f) => join(oracle, g, f)));
}
const files = [];
for (let r = 0; files.length < groups.reduce((a, g) => a + g.length, 0); r++) for (const g of groups) if (g[r]) files.push(g[r]);

// tabela de faces (as réplicas TS/C# reabrem pelo provedor do sistema)
const faces = [], faceIds = new Map();
const faceId = (f) => {
  if (!f) return -1;
  let id = faceIds.get(f.key);
  if (id === undefined) {
    id = faces.length;
    faceIds.set(f.key, id);
    faces.push({ key: f.key, family: f.family, bold: !!f.bold, italic: !!f.italic, synthBold: !!f.synthBold, synthItalic: !!f.synthItalic });
  }
  return id;
};

let ctxNow = null;
const glyph = (g) => {
  const o = { k: g.kind, ch: g.ch ?? '', cp: g.cp ?? 0, adv: g.adv, asc: g.asc, desc: g.desc, off: g.offset, br: !!g.breakAfter, sz: g.sizePt, f: faceId(g.face) };
  if (g.nAsc !== undefined) o.nAsc = g.nAsc;
  if (g.nDesc !== undefined) o.nDesc = g.nDesc;
  if (g.glue) o.glue = 1;
  if (g.clear) o.clear = 1;
  if (g.hr !== undefined) o.hr = g.hr;
  if (g.ptab) o.ptab = { a: g.ptab.alignment, l: g.ptab.leader ?? null };
  if (g.feShrink !== undefined) o.fe = g.feShrink;
  if (g.fixed) o.fixed = 1;
  if (g.hang) o.hang = 1;
  if (g.hyph) o.hyph = 1;
  if (g.style?.underline && g.style.underline.val !== 'none') o.ul = 1;
  // pré-cálculos de fonte para o Rust: largura do hífen (pontos de hifenização) e métricas
  if (g.hyph || g.kind === 'shy') o.hyW = advanceTwips(ctxNow, g.face, 0x2d, g.sizePt);
  if (g.kind === 'obj' || g.hr !== undefined) { const m = faceMetrics(g.face.font); o.fa = m.ascent; o.fd = m.descent; }
  return o;
};

const paras = [], events = [], lines = [];
const ids = new WeakMap();
let docIdx = -1;
const register = (br) => {
  let id = ids.get(br);
  if (id !== undefined) return id;
  id = paras.length;
  ids.set(br, id);
  const p = br.p;
  const mm = faceMetrics(p.markFace.font);
  const mk = p.marker;
  const s = ctxNow.fmt.settings;
  paras.push({
    doc: docIdx, grid: br.grid ?? null,
    settings: { defaultTabStop: s.defaultTabStop, compatibilityMode: s.compatibilityMode, doNotUseIndentAsNumberingTabStop: !!s.compat['doNotUseIndentAsNumberingTabStop'],
      splitPgBreakAndParaMark: !!s.compat['splitPgBreakAndParaMark'], consecutiveHyphenLimit: s.consecutiveHyphenLimit ?? 0, hyphenationZone: s.hyphenationZone ?? null },
    glyphs: p.glyphs.map(glyph),
    tabs: p.tabs.map((t) => ({ val: t.val, pos: t.pos, leader: t.leader ?? null })),
    firstLine: p.firstLine, indLeft: p.indLeft, indRight: p.indRight, jc: p.jc, lineRule: p.lineRule, line: p.line,
    marker: mk ? { jc: mk.jc ?? 'left', width: mk.width, suff: mk.suff ?? 'tab', glyphs: mk.glyphs.map(glyph),
      spaceW: mk.suff === 'space' ? advanceTwips(ctxNow, mk.glyphs[0]?.face ?? p.markFace, 0x20, mk.glyphs[0]?.sizePt ?? p.markStyle.sizePt) : 0 } : null,
    markFace: faceId(p.markFace), markAsc: mm.ascent, markDesc: mm.descent,
    markStyle: { sizePt: p.markStyle.sizePt, shift: p.markStyle.shift ?? 0, vertAlign: p.markStyle.rpr.vertAlign ?? null, sz: p.markStyle.rpr.sz ?? null, textShadow: p.markStyle.rpr.textShadow ?? null, shadow: shadowExtent(p.markStyle) },
  });
  events.push(['new', id]);
  return id;
};
const pg = (q, p, markerGlyphs) => {
  const gi = p.glyphs.indexOf(q.g);
  const mi = gi < 0 && markerGlyphs ? markerGlyphs.indexOf(q.g) : -1;
  // índice no parágrafo; -1 = hífen automático (sintético, do glifo hyphAt); -2-k = k-ésimo glifo do marcador
  return [gi >= 0 ? gi : mi >= 0 ? -2 - mi : -1, q.x, q.w, q.leader ?? null];
};
const origNext = LineBreaker.prototype.next, origReset = LineBreaker.prototype.reset;
LineBreaker.prototype.next = function (frame) {
  ctxNow = this.ctx;
  const id = register(this);
  const l = origNext.call(this, frame);
  const p = this.p;
  events.push(['next', id, { left: frame.left, right: frame.right, ...(frame.gap ? { gap: true } : {}), ...(frame.next ? { next: { left: frame.next.left, right: frame.next.right } } : {}) }]);
  lines.push({
    start: l.start, end: l.end, width: l.width, xStart: l.xStart, asc: l.asc, desc: l.desc, height: l.height, baseline: l.baseline, last: l.last,
    breakAfter: l.breakAfter ?? null, clear: !!l.clear, gapTab: l.gapTab ?? null, spanned: !!l.spanned, hr: !!l.hrFrame,
    placed: l.placed.map((q) => pg(q, p, p.marker?.glyphs)), marker: l.marker ? l.marker.map((q) => pg(q, p, p.marker.glyphs)) : null,
  });
  return l;
};
LineBreaker.prototype.reset = function (m) {
  ctxNow = this.ctx;
  const id = register(this);
  events.push(['reset', id, { pos: m.pos, lineIdx: m.lineIdx, trailingBreak: m.trailingBreak, hyphenRun: m.hyphenRun }]);
  return origReset.call(this, m);
};

const docs = [];
let skipped = 0, failed = 0;
for (const file of files.slice(0, max)) {
  if (paras.reduce((a, p) => a + p.glyphs.length, 0) >= budget) break;
  const bytes = new Uint8Array(readFileSync(file));
  let pkg;
  try { pkg = core.DocxPackage.load(bytes); } catch { failed++; continue; }
  let embedded = [];
  try { embedded = core.readEmbeddedFonts(pkg); } catch { /* sem fontes embutidas legíveis */ }
  if (embedded.length) { skipped++; continue; }
  const mark = { paras: paras.length, events: events.length, lines: lines.length, faces: faces.length };
  docIdx = docs.length;
  try {
    eng.layoutDocument(core.readFullDocument(pkg), provider, opts);
    // documento longo demais: fica de fora para o orçamento cobrir mais grupos do oráculo
    if (paras.slice(mark.paras).reduce((n, p) => n + p.glyphs.length, 0) > 6000) {
      paras.length = mark.paras; events.length = mark.events; lines.length = mark.lines;
      skipped++;
      continue;
    }
    docs.push(file.slice(oracle.length + 1).replace(/\\/g, '/'));
  } catch {
    // documento que o motor recusa: desfaz o que capturou dele
    paras.length = mark.paras; events.length = mark.events; lines.length = mark.lines;
    failed++;
  }
}
mkdirSync(join(here, '../data'), { recursive: true });
const out = { opts: { advanceEm: 'nominal', advanceRound: 'word', justifyShrink: { ratio: 0.25, expandRatio: 0.5 } }, docs, faces, paras, events, lines };
writeFileSync(join(here, '../data/lines.json'), JSON.stringify(out));
const glyphs = paras.reduce((a, p) => a + p.glyphs.length, 0);
console.log(`${docs.length} documentos (${skipped} fora por fontes embutidas ou tamanho, ${failed} com erro), ${paras.length} parágrafos, ${glyphs} glifos, ${lines.length} linhas, ${events.length} eventos, ${faces.length} faces`);
