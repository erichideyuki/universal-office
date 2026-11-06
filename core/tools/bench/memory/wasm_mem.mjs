// wasm-in-Node side of the memory benchmark: ONE process per document (run.mjs spawns it with --expose-gc).
// Drives the Write exports through js/write.mjs as the viewer does (env + fonts, open, layout, page messages,
// optionally the PDF) and records, after each step, the module's live bytes (uo_memory_live_bytes, build with
// feature `count`) and memory.buffer.byteLength (the high-water: wasm memory never shrinks).
//   node --expose-gc wasm_mem.mjs <file.docx> <module.wasm> [--phone] [--pdf]
// Prints one JSON line.
import { readFileSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { writeImports, bindWrite } from '../../../crates/uo-wasm/js/write.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../..');
const [file, wasmPath] = process.argv.slice(2);
const phone = process.argv.includes('--phone');
const withPdf = process.argv.includes('--pdf');
if (typeof gc !== 'function') throw new Error('run with node --expose-gc');

const OPEN_FONTS = [
  ['Liberation Sans', 'site/assets/fonts', 'liberationsans'],
  ['Liberation Serif', 'shared/fonts/files', 'liberationserif'],
  ['Liberation Mono', 'shared/fonts/files', 'liberationmono'],
  ['Carlito', 'shared/fonts/files', 'carlito'],
  ['Caladea', 'shared/fonts/files', 'caladea'],
];
const VARIANTS = ['regular', 'bold', 'italic', 'boldItalic'];
const fontFiles = OPEN_FONTS.map(([family, dir, stem]) => [family, ['regular', 'bold', 'italic', 'bolditalic'].map((s) => readFileSync(join(repo, dir, `${stem}-${s}.ttf`)))]);
const jsFaceBytes = fontFiles.reduce((n, [, fs]) => n + fs.reduce((m, b) => m + b.length, 0), 0);
const hyphDir = join(repo, 'core/crates/uo-fonts/data/hyphen');
const hyphText = (id) => readFileSync(join(hyphDir, `${id}.txt`), 'utf8');
const docx = new Uint8Array(readFileSync(file));
const wasm = readFileSync(wasmPath);
const OPTS = { generation: 1, now: 1_768_478_400_000, tzOffsetMin: 0, hyphenationLanguages: ['pt-BR', 'en', 'es'] };
const MB = 1024 * 1024;

async function session(tag) {
  const { imports, host } = writeImports();
  const instance = (await WebAssembly.instantiate(wasm, imports)).instance;
  const w = instance.exports;
  const uo = bindWrite(w, host);
  const live = () => w.uo_memory_live_bytes();
  const len = () => w.memory.buffer.byteLength;
  const steps = {};
  const mark = (name) => { steps[name] = { live: live(), bytes: len(), rss: process.memoryUsage.rss() }; };
  mark('instantiated');
  const env = uo.newEnv({ phone });
  for (const [family, files] of fontFiles) env.pushFamily(family, { source: 'installed', embeddable: true, faces: files.map((b, i) => ({ variant: VARIANTS[i], bytes: b })) });
  mark('fonts');
  const doc = uo.open(docx);
  mark('open');
  for (const id of doc.hyphenationPatterns(env)) env.registerHyphenation(hyphText(id));
  mark('hyphenation');
  // first layout (parses the faces into the env's cache), then dropped: the baseline for the retained layout
  let l = doc.layout(env, OPTS);
  const pages = l.pageCount;
  mark('layout1');
  l.free();
  mark('layout1_freed');
  l = doc.layout(env, OPTS);
  mark('layout2');
  const n = pages;
  let sum = 0, got = 0;
  const first = Math.min(5, n);
  for (let i = 0; i < first; i++) { sum += l.encodePage(i).length; got++; }
  mark('encode_visible');
  for (let i = first; i < n; i++) { sum += l.encodePage(i).length; got++; }
  mark('encode_all');
  const nav = l.navigation();
  l.runBoxes('links');
  mark('navigation');
  let pdfBytes = 0;
  if (withPdf) { pdfBytes = l.pdf({ compress: true }).length; mark('pdf'); }
  l.free();
  mark('layout_freed');
  doc.free();
  mark('doc_freed');
  env.free();
  mark('env_freed');
  const weak = new WeakRef(w.memory);
  return { steps, pages, encodedBytes: sum, links: nav.links.length, pdfBytes, weak };
}

// an asynchronous major GC is what lets V8 release a dropped instance (a synchronous gc() leaves it)
async function settleGc() { for (let i = 0; i < 3; i++) await gc({ type: 'major', execution: 'async' }); }
const proc = async () => { await settleGc(); const m = process.memoryUsage(); return { rss: m.rss, arrayBuffers: m.arrayBuffers, heapUsed: m.heapUsed }; };

const before = await proc();
const extBefore = process.memoryUsage().external;
const a = await session('first');
const during = await proc();
const stepsA = a.steps;
// drop the instance: nothing of it may stay reachable (the wrapper classes and registries are closed over by `session`)
const summary = { pages: a.pages, encodedBytes: a.encodedBytes, links: a.links, pdfBytes: a.pdfBytes };
const weakA = a.weak;
a.weak = null;
let collected = false;
for (let i = 0; i < 20 && !collected; i++) {
  await settleGc();
  await new Promise((r) => setTimeout(r, 50));
  collected = weakA.deref() === undefined;
}
const afterDrop = await proc();
// a fresh instance, same flow: its high-water starts again from the initial size
const b = await session('second');
const afterSecond = await proc();
const out = {
  docx_bytes: docx.length,
  profile: phone ? 'phone' : 'desktop',
  wasm_module_bytes: wasm.length,
  js_face_bytes: jsFaceBytes,
  ...summary,
  instance_collected: collected,
  steps: stepsA,
  second_instance_steps: { instantiated: b.steps.instantiated, layout2: b.steps.layout2, encode_all: b.steps.encode_all, env_freed: b.steps.env_freed },
  node: {
    before_arrayBuffers: before.arrayBuffers, during_arrayBuffers: during.arrayBuffers, after_drop_arrayBuffers: afterDrop.arrayBuffers, after_second_arrayBuffers: afterSecond.arrayBuffers,
    before_rss: before.rss, during_rss: during.rss, after_drop_rss: afterDrop.rss, after_second_rss: afterSecond.rss,
  },
  highwater_bytes: stepsA.encode_all.bytes,
  highwater_final: stepsA[withPdf ? 'pdf' : 'navigation'].bytes,
  live_retained: {
    faces: stepsA.layout1_freed.live - stepsA.hyphenation.live,
    layout: stepsA.layout2.live - stepsA.layout1_freed.live,
    encode_all: stepsA.encode_all.live - stepsA.layout2.live,
    document: stepsA.open.live - stepsA.fonts.live,
    env: stepsA.fonts.live - stepsA.instantiated.live,
  },
};
console.log(JSON.stringify(out));
void readdirSync; void MB;
