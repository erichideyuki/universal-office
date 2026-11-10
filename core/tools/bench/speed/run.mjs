// Speed benchmark main x rust-core, orchestrator (record core/docs/modules/uo-docx-layout.md,
// "Benchmarks main x rust-core - speed"). ONE Node process drives three engines over the same documents, interleaved
// per document and per round (the start engine rotates every round), so machine load hits all of them alike:
//   ts      the frozen TS engine (UO_TS_ROOT, dist built): DocxPackage.load + readFullDocument + readEmbeddedFonts,
//           layoutDocument + pdfNavigation, renderPdf (compressed)
//   wasm    the shipped module (feature write, profile wasm) through js/write.mjs in this Node: open, layout
//           (navigation inside, as the native pipeline), pdf({ compress: true })
//   native  core/crates/uo-ffi/examples/bench_speed (a persistent child process, one stdin line per iteration)
// Stages: open (read), layout (+ navigation), pdf. One untimed warm-up iteration per engine and document.
//
//   export UO_TS_ROOT=<frozen TS checkout, dist built>  CARGO_TARGET_DIR=<own target dir>
//   node core/tools/bench/speed/run.mjs [--no-build] [--rounds 9] [--docs captures,typical,tables,images,pages100,pages500]
//        [--engines ts,wasm,native] [--out <dir>] [--tag <name>]
process.env.TZ = 'UTC'; // the TS formats `now` in the local zone; the Rust side is pinned to offset 0 (as the capture tools)
import { spawn, execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, writeFileSync, mkdirSync, copyFileSync, existsSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createInterface } from 'node:readline';
import { buildDocx, CLASSES, loadFflate } from '../memory/gen_docs.mjs';
import { loadWrite } from '../../../crates/uo-wasm/js/write.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const core = resolve(here, '../../..');
const repo = resolve(core, '..');
const args = process.argv.slice(2);
const opt = (k, d) => { const i = args.indexOf(k); return i >= 0 ? args[i + 1] : d; };
const root = process.env.UO_TS_ROOT && resolve(process.env.UO_TS_ROOT);
if (!root || !existsSync(join(root, 'write/packages/docx-layout/dist/index.js'))) throw new Error('set UO_TS_ROOT to the frozen TS checkout (dist built)');
const target = resolve(process.env.CARGO_TARGET_DIR ?? join(core, 'target/agent-bench'));
const exe = process.platform === 'win32' ? '.exe' : '';
const engines = new Set(opt('--engines', 'ts,wasm,native').split(','));
const sets = opt('--docs', 'captures,typical,tables,images,pages100,pages500').split(',');
const roundsOpt = Number(opt('--rounds', 9));
const outDir = resolve(opt('--out', join(here, 'results')));
const tag = opt('--tag', new Date().toISOString().slice(0, 10));
mkdirSync(outDir, { recursive: true });
const docsDir = join(target, 'docs');
mkdirSync(docsDir, { recursive: true });

if (!args.includes('--no-build')) {
  const env = { ...process.env, CARGO_TARGET_DIR: target };
  execFileSync('cargo', ['build', '--release', '-j', '4', '-p', 'uo-ffi', '--example', 'bench_speed'], { cwd: core, env, stdio: 'inherit' });
  execFileSync('cargo', ['build', '-p', 'uo-wasm', '--features', 'write', '--target', 'wasm32-unknown-unknown', '--profile', 'wasm', '-j', '4'], { cwd: core, env, stdio: 'inherit' });
}
const wasmPath = join(target, 'wasm32-unknown-unknown/wasm/uo_wasm.wasm');

// ── the documents ─────────────────────────────────────────────────────────────────────────────
const docs = [];
const capRoot = join(core, 'crates/uo-docx-layout/tests/captures');
for (const s of sets) {
  if (s === 'captures') {
    for (const fam of ['f7a', 'f7b', 'f7c', 'f7d', 'f8']) {
      const d = join(capRoot, fam, 'docs');
      for (const n of readdirSync(d).sort()) if (n.endsWith('.docx')) docs.push({ name: `${fam}/${n.slice(0, -5)}`, cls: 'captures', path: join(d, n) });
    }
  } else if (CLASSES[s]) {
    const p = join(docsDir, `${s}.docx`);
    if (!existsSync(p)) writeFileSync(p, buildDocx({ fflate: await loadFflate(root), ...CLASSES[s] }));
    docs.push({ name: s, cls: s, path: p });
  } else throw new Error(`unknown doc set ${s}`);
}
const roundsOf = (d) => (d.cls === 'pages500' ? Math.min(roundsOpt, 3) : d.cls === 'pages100' ? Math.min(roundsOpt, 5) : d.cls === 'captures' ? roundsOpt : Math.min(roundsOpt, 7));

// ── engines ───────────────────────────────────────────────────────────────────────────────────
const importFrom = (rel) => import(pathToFileURL(join(root, rel)).href);
const ms = (f) => { const t = performance.now(); const r = f(); return [performance.now() - t, r]; };
const fnv = (b) => { let h = 0x811c9dc5; for (let i = 0; i < b.length; i++) { h ^= b[i]; h = Math.imul(h, 0x01000193) >>> 0; } return h.toString(16).padStart(8, '0'); };
const NOW = new Date(Date.UTC(2026, 0, 15, 12, 0, 0));
const impl = {};

if (engines.has('ts')) {
  process.env.WINDIR = join(here, '__no_system_fonts__');
  process.env.LOCALAPPDATA = join(here, '__no_system_fonts__');
  process.env.EXTRA_FONT_DIRS = [join(root, 'shared/fonts/files'), join(root, 'site/assets/fonts')].join(';');
  const dc = await importFrom('write/packages/docx-core/dist/index.js');
  const eng = await importFrom('write/packages/docx-layout/dist/index.js');
  const { createNodeFontProvider } = await importFrom('write/tools/corpus-lab/node-fonts.mjs');
  const provider = await createNodeFontProvider();
  await eng.loadHyphenation(eng.HYPHENATION_PATTERN_IDS);
  impl.ts = (d, compress = true) => {
    const bytes = new Uint8Array(d.bytes);
    const [open, st] = ms(() => {
      const pkg = dc.DocxPackage.load(bytes);
      const full = dc.readFullDocument(pkg);
      let embedded = [];
      try { embedded = dc.readEmbeddedFonts(pkg); } catch { /* none */ }
      return { full, embedded };
    });
    const [layout, lr] = ms(() => {
      const prov = eng.withEmbeddedFonts(provider, st.embedded, (b) => new eng.SfntFont(b));
      const res = eng.layoutDocument(st.full, prov, { now: NOW });
      return { res, navigation: eng.pdfNavigation(st.full, res) };
    });
    const [pdf, out] = ms(() => eng.renderPdf(lr.res, { compress, navigation: lr.navigation }));
    return { open, layout, pdf, pages: lr.res.pages.length, pdf_len: out.length, fnv: fnv(out) };
  };
}

if (engines.has('wasm')) {
  const wasm = new Uint8Array(readFileSync(wasmPath));
  const { uo } = await loadWrite({ engine: 'wasm', wasm });
  const OPEN = [['Liberation Sans', 'site/assets/fonts', 'liberationsans'], ['Liberation Serif', 'shared/fonts/files', 'liberationserif'], ['Liberation Mono', 'shared/fonts/files', 'liberationmono'], ['Carlito', 'shared/fonts/files', 'carlito'], ['Caladea', 'shared/fonts/files', 'caladea']];
  const V = ['regular', 'bold', 'italic', 'boldItalic'];
  const env = uo.newEnv();
  for (const [family, dir, stem] of OPEN) {
    env.pushFamily(family, { source: 'installed', embeddable: true, faces: ['regular', 'bold', 'italic', 'bolditalic'].map((s, i) => ({ variant: V[i], bytes: new Uint8Array(readFileSync(join(repo, dir, `${stem}-${s}.ttf`))) })) });
  }
  const registered = new Set();
  const OPTS = { generation: 1, now: 1_768_478_400_000, tzOffsetMin: 0, hyphenationLanguages: ['pt-BR', 'en', 'es'] };
  const hyphDir = join(core, 'crates/uo-fonts/data/hyphen');
  impl.wasm = (d, compress = true) => {
    const [open, doc] = ms(() => uo.open(new Uint8Array(d.bytes)));
    for (const id of doc.hyphenationPatterns(env)) if (!registered.has(id)) { env.registerHyphenation(readFileSync(join(hyphDir, `${id}.txt`), 'utf8')); registered.add(id); }
    const [layout, l] = ms(() => doc.layout(env, OPTS));
    const [pdf, chunks] = ms(() => l.pdf({ compress }).chunks);
    const len = chunks.reduce((n, c) => n + c.length, 0);
    const all = new Uint8Array(len); let at = 0; for (const c of chunks) { all.set(c, at); at += c.length; }
    const pages = l.pageCount;
    l.free(); doc.free();
    return { open, layout, pdf, pages, pdf_len: len, fnv: fnv(all) };
  };
}

let child = null; let lines = null;
if (engines.has('native')) {
  child = spawn(join(target, 'release/examples/bench_speed' + exe), [], { stdio: ['pipe', 'pipe', 'inherit'] });
  lines = createInterface({ input: child.stdout })[Symbol.asyncIterator]();
  impl.native = async (d, compress = true) => {
    child.stdin.write(`run\t${d.path}\t${compress ? 1 : 0}\n`);
    const { value } = await lines.next();
    return JSON.parse(value);
  };
}

// ── the run ───────────────────────────────────────────────────────────────────────────────────
const names = ['ts', 'wasm', 'native'].filter((n) => engines.has(n));
const median = (xs) => { const s = [...xs].sort((a, b) => a - b); return s.length % 2 ? s[s.length >> 1] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2; };
const results = [];
const STAGES = ['open', 'layout', 'pdf'];
for (const d of docs) {
  d.bytes = readFileSync(d.path);
  const rows = Object.fromEntries(names.map((n) => [n, []]));
  const first = {};
  for (const n of names) first[n] = await impl[n](d); // warm-up (faces, JIT, caches)
  const R = roundsOf(d);
  for (let r = 0; r < R; r++) {
    for (let k = 0; k < names.length; k++) {
      const n = names[(r + k) % names.length];
      rows[n].push(await impl[n](d));
    }
  }
  const sum = {};
  for (const n of names) {
    sum[n] = { first: { ...first[n] }, pages: rows[n][0].pages, pdf_len: rows[n][0].pdf_len, fnv: rows[n][0].fnv };
    for (const s of [...STAGES, 'total']) {
      const xs = rows[n].map((x) => (s === 'total' ? x.open + x.layout + x.pdf : x[s]));
      sum[n][s] = { min: Math.min(...xs), median: median(xs) };
    }
  }
  // untimed equality pass: uncompressed PDFs (the compressed bytes depend on the deflate implementation; the parity gates compare raw)
  const raw = {};
  for (const n of names) { const x = await impl[n](d, false); raw[n] = { pdf_len: x.pdf_len, fnv: x.fnv, pages: x.pages }; sum[n].raw = raw[n]; }
  results.push({ name: d.name, cls: d.cls, docx_bytes: d.bytes.length, rounds: R, engines: sum });
  const f = (n, s) => (sum[n] ? sum[n][s].median.toFixed(1) : '-');
  console.error(`${d.name.padEnd(14)} ${sum[names[0]].pages}p  total median ms ${names.map((n) => `${n} ${f(n, 'total')}`).join('  ')}`);
}
child?.stdin.write('quit\n');
child?.stdin.end();

// ── summary tables ────────────────────────────────────────────────────────────────────────────
const classes = [...new Set(results.map((r) => r.cls))];
const rowsOf = (cls) => results.filter((r) => r.cls === cls);
const sumOf = (rs, n, s, k = 'median') => rs.reduce((a, r) => a + r.engines[n][s][k], 0);
const fmt = (x) => (x >= 100 ? x.toFixed(0) : x.toFixed(1));
const ratio = (a, b) => (b ? (a / b).toFixed(2) : '-');
const md = [];
md.push(`| class | docs | pages | stage | ${names.map((n) => `${n} (ms, sum of medians)`).join(' | ')} | ${names.includes('ts') ? names.filter((n) => n !== 'ts').map((n) => `${n} / ts`).join(' | ') : ''} |`);
md.push(`|${'---|'.repeat(4 + names.length + (names.includes('ts') ? names.length - 1 : 0))}`);
for (const cls of classes) {
  const rs = rowsOf(cls);
  const pages = rs.reduce((a, r) => a + r.engines[names[0]].pages, 0);
  for (const s of ['open', 'layout', 'pdf', 'total']) {
    const v = names.map((n) => sumOf(rs, n, s));
    md.push(`| ${cls} | ${rs.length} | ${pages} | ${s} | ${v.map(fmt).join(' | ')} | ${names.includes('ts') ? names.filter((n) => n !== 'ts').map((n) => ratio(v[names.indexOf(n)], v[names.indexOf('ts')])).join(' | ') : ''} |`);
  }
}
// per-document ratio distribution (median of per-document medians of the total)
const dist = {};
for (const n of names.filter((x) => x !== 'ts')) {
  if (!names.includes('ts')) break;
  const rr = results.filter((r) => r.cls === 'captures').map((r) => r.engines[n].total.median / r.engines.ts.total.median).sort((a, b) => a - b);
  dist[n] = { docs: rr.length, p10: rr[Math.floor(rr.length * 0.1)], median: rr[rr.length >> 1], p90: rr[Math.floor(rr.length * 0.9)], max: rr.at(-1), slower: rr.filter((x) => x > 1).length };
}
// output agreement
const mismatches = [];
for (const r of results) {
  const e = r.engines;
  if (e.native && e.wasm && (e.native.raw.fnv !== e.wasm.raw.fnv)) mismatches.push(`${r.name}: raw PDF native ${e.native.raw.pdf_len}/${e.native.raw.fnv} vs wasm ${e.wasm.raw.pdf_len}/${e.wasm.raw.fnv}`);
  if (e.ts && e.native && (e.ts.raw.fnv !== e.native.raw.fnv || e.ts.pages !== e.native.pages)) mismatches.push(`${r.name}: raw PDF ts ${e.ts.pages}p ${e.ts.raw.pdf_len}/${e.ts.raw.fnv} vs native ${e.native.pages}p ${e.native.raw.pdf_len}/${e.native.raw.fnv}`);
}
const report = { date: new Date().toISOString(), node: process.version, engines: names, wasm: existsSync(wasmPath) ? { bytes: readFileSync(wasmPath).length } : null, ratioDistribution: dist, outputMismatches: mismatches, results };
writeFileSync(join(outDir, `speed-${tag}.json`), JSON.stringify(report, null, 1));
writeFileSync(join(outDir, `speed-${tag}.md`), [`# Speed ${tag}`, '', ...md, '', `per-document total ratio (captures): ${JSON.stringify(dist)}`, '', `output mismatches (${mismatches.length}):`, ...mismatches.map((m) => `- ${m}`)].join('\n'));
console.log(md.join('\n'));
console.log('ratio distribution (captures):', JSON.stringify(dist));
console.log(`output mismatches: ${mismatches.length}`);
for (const m of mismatches.slice(0, 40)) console.log(' ', m);
process.exit(0);
