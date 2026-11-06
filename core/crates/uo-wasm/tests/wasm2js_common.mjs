// Shared by write_wasm2js.test.mjs (Node) and browser/wasm2js_run.mjs (Chromium, WebKit): the viewer session over the
// committed fixtures, collected as bytes, so two engines (WebAssembly, wasm2js) can be compared exactly, and timed.
// `read(path)` (repository-relative, forward slashes) -> Promise<Uint8Array> is the only host dependency.
import { fnv } from '../js/render.mjs';

const OPEN_FONTS = [
  ['Liberation Sans', 'site/assets/fonts', 'liberationsans'],
  ['Liberation Serif', 'shared/fonts/files', 'liberationserif'],
  ['Liberation Mono', 'shared/fonts/files', 'liberationmono'],
  ['Carlito', 'shared/fonts/files', 'carlito'],
  ['Caladea', 'shared/fonts/files', 'caladea'],
];
const VARIANTS = ['regular', 'bold', 'italic', 'boldItalic'];
export const FIXTURES = Array.from({ length: 12 }, (_, i) => `f8__7__${i}`);
/** the native checksums of the PDF (tests/write_native.rs, as in write_api.test.mjs) */
export const PINNED_PDF = { f8__7__0: 'c28b3c53', f8__7__3: 'a2cf1bd6', f8__7__10: '88d31b5b' };
export const OPTS = { generation: 1, now: 1_768_478_400_000, tzOffsetMin: 0, hyphenationLanguages: ['pt-BR', 'en', 'es'] };

export async function loadAssets(read, fixtures = FIXTURES) {
  const fonts = [];
  for (const [family, dir, stem] of OPEN_FONTS) {
    const faces = [];
    for (const [i, s] of ['regular', 'bold', 'italic', 'bolditalic'].entries()) faces.push({ variant: VARIANTS[i], bytes: await read(`${dir}/${stem}-${s}.ttf`) });
    fonts.push([family, faces]);
  }
  const docs = {};
  for (const k of fixtures) docs[k] = await read(`core/crates/uo-docx-layout/tests/captures/f8/docs/${k}.docx`);
  return { fonts, docs };
}

export function newEnv(uo, assets) {
  const env = uo.newEnv();
  for (const [family, faces] of assets.fonts) env.pushFamily(family, { source: 'installed', embeddable: true, faces });
  return env;
}

export const concat = (chunks) => { const out = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0)); let at = 0; for (const c of chunks) { out.set(c, at); at += c.length; } return out; };
const te = new TextEncoder();
const json = (v) => te.encode(JSON.stringify(v, (_k, x) => (typeof x === 'number' && !Number.isFinite(x) ? String(x) : x)));

/** everything a viewer reads from a layout, as bytes: page messages (render and Write sections), annotations, navigation, link boxes, PDF (compressed and not) */
export function collect(uo, assets, key) {
  const env = newEnv(uo, assets);
  const doc = uo.open(assets.docs[key]);
  const layout = doc.layout(env, OPTS);
  const n = layout.pageCount;
  const out = { pages: n, messages: [], sections: [] };
  for (let i = 0; i < n; i++) out.messages.push(layout.paint.encodePage(i).take().data.slice());
  const l2 = doc.layout(env, OPTS);
  for (let i = 0; i < n; i++) out.sections.push(l2.encodePage(i).slice());
  l2.free();
  out.annotations = json(layout.annotations());
  out.navigation = json(layout.navigation());
  out.links = json(layout.runBoxes('links'));
  out.pageInfo = json(Array.from({ length: n }, (_, i) => layout.pageInfo(i)));
  out.face0 = layout.faceBytes(0);
  out.pdf = concat(layout.pdf().chunks);
  out.raw = concat(layout.pdf({ compress: false, chunk: 4096 }).chunks);
  layout.free(); doc.free(); env.free();
  return out;
}

/** the parts of `collect`, flattened to named byte strings (for comparison and checksums) */
export function parts(c) {
  return { messages: concat(c.messages), sections: concat(c.sections), annotations: c.annotations, navigation: c.navigation, links: c.links, pageInfo: c.pageInfo, face0: c.face0 ?? new Uint8Array(0), pdf: c.pdf, raw: c.raw };
}
export function digest(c) {
  return Object.entries(parts(c)).map(([k, v]) => `${k}=${fnv(v)}`).join(' ');
}
export function firstDifference(a, b) {
  const pa = parts(a), pb = parts(b);
  for (const k of Object.keys(pa)) {
    const x = pa[k], y = pb[k];
    if (x.length !== y.length) return `${k}: length ${x.length} vs ${y.length}`;
    for (let i = 0; i < x.length; i++) if (x[i] !== y[i]) return `${k}: byte ${i}`;
  }
  return null;
}

/** milliseconds of layout, all pages, PDF for one fixture (env and document prepared outside) */
export function timeOnce(env, doc, now = () => performance.now()) {
  const t0 = now();
  const layout = doc.layout(env, OPTS);
  const t1 = now();
  for (let i = 0; i < layout.pageCount; i++) { layout.paint.encodePage(i).take(); layout.encodePage(i); }
  const t2 = now();
  layout.pdf();
  const t3 = now();
  layout.free();
  return { layout: t1 - t0, pages: t2 - t1, pdf: t3 - t2, total: t3 - t0 };
}
export const median = (xs) => [...xs].sort((a, b) => a - b)[xs.length >> 1];
export function timeFixture(uo, assets, key, runs = 5, now) {
  const env = newEnv(uo, assets);
  const doc = uo.open(assets.docs[key]);
  timeOnce(env, doc, now); // warm-up (faces parsed, JIT)
  const rows = Array.from({ length: runs }, () => timeOnce(env, doc, now));
  doc.free(); env.free();
  const m = {};
  for (const k of ['layout', 'pages', 'pdf', 'total']) m[k] = median(rows.map((r) => r[k]));
  return m;
}
