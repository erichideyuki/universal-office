// The Write exports of uo-wasm through js/write.mjs (milestone 1, uo-bindings.md): a viewer session
// over three committed fixture documents (open fonts) with the checksums of tests/write_native.rs pinned
// — the wasm bytes equal the native bytes (page messages, navigation, link boxes, the PDF compressed
// and not); errors; the memory gate at the boundary: M4 (10,000 environments, documents and layouts
// through free() and through the FinalizationRegistry; live bytes back exactly) and M2 (repeated
// sessions: live bytes flat, memory.buffer stops growing). The same file runs in WebKit through
// browser/run.mjs (without the finalizer test, which needs gc()).
//   cargo build -p uo-wasm --features count,write --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> node --expose-gc --test crates/uo-wasm/tests/write_api.test.mjs
// UO_WRITE_M4 (default 10000) and UO_WRITE_SOAK (default 30) size the memory tests.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { writeImports, bindWrite, WriteError } from '../js/write.mjs';
import { fnv } from '../js/render.mjs';
import { decodePage, newTables } from '../js/gen/render.ts';
import { decodeWritePage, newTables as newWriteTables } from '../js/gen/write.ts';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../..');
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const bytes = readFileSync(wasmPath);
const has = WebAssembly.Module.imports(new WebAssembly.Module(bytes)).some((i) => i.module === 'uo_host');
const skip = has ? false : 'this build has no write exports (--features write)';
const { imports, host } = writeImports();
const instance = has ? (await WebAssembly.instantiate(bytes, imports)).instance : null;
const w = instance?.exports;
const uo = has ? bindWrite(w, host) : null;
const counting = has && w.uo_memory_live_bytes() >= 0;
const M4 = Number(process.env.UO_WRITE_M4 ?? 10_000);
const SOAK = Number(process.env.UO_WRITE_SOAK ?? 30);
const live = () => w.uo_memory_live_bytes();
const settle = async () => {
  for (let i = 0; i < 5; i++) { globalThis.gc?.(); await new Promise((ok) => setTimeout(ok, 10)); }
  return live();
};

// the open fonts of the repository, pushed as the viewer's host would (tests/write_native.rs OPEN_FONTS)
const OPEN_FONTS = [
  ['Liberation Sans', 'site/assets/fonts', 'liberationsans'],
  ['Liberation Serif', 'shared/fonts/files', 'liberationserif'],
  ['Liberation Mono', 'shared/fonts/files', 'liberationmono'],
  ['Carlito', 'shared/fonts/files', 'carlito'],
  ['Caladea', 'shared/fonts/files', 'caladea'],
];
const fontFiles = has ? OPEN_FONTS.map(([family, dir, stem]) => [family, ['regular', 'bold', 'italic', 'bolditalic'].map((s) => readFileSync(join(repo, dir, `${stem}-${s}.ttf`)))]) : [];
const VARIANTS = ['regular', 'bold', 'italic', 'boldItalic'];
const withFonts = (env) => {
  for (const [family, files] of fontFiles) env.pushFamily(family, { source: 'installed', embeddable: true, faces: files.map((b, i) => ({ variant: VARIANTS[i], bytes: b })) });
  return env;
};
const fixture = (key) => readFileSync(join(repo, `core/crates/uo-docx-layout/tests/captures/f8/docs/${key}.docx`));
const OPTS = { generation: 1, now: 1_768_478_400_000, tzOffsetMin: 0, hyphenationLanguages: ['pt-BR', 'en', 'es'] };
const hex = (n) => fnv(n);
const concat = (chunks) => { const out = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0)); let at = 0; for (const c of chunks) { out.set(c, at); at += c.length; } return out; };

// printed by `cargo test -p uo-wasm --features write,count --test write_native -- --nocapture`
const PINNED = {
  // WordCraft study (fields, C1b and B22; record uo-docx-layout.md "WordCraft study — fields"): page 2 only — multiple spacing
  // over the largest OWN text height, and `PAGE \* upperLetter` in header2 is Word's unknown-switch error text (`upperLetter`
  // is no field keyword); bit 21 gives the old pages back (was ea592122/8cea474e/…/c28b3c53/28d8e9a7)
  f8__7__0: 'pages=3 messages=05fc37d7 sections=58a6a885 annotations=4ceffb56 navigation=77ac1ff2 links=fed3646f pdf=d8c35a91 raw=a3d68b20',
  // WordCraft study (page, B10/B11): its line-relative box on page 2 sits on its anchor line, measured after the earlier
  // float Word lays the page out again around (uo-docx-layout word_wc_page.rs; was 2d10de80/0f5959f7/…/8484558e)
  f8__7__3: 'pages=4 messages=0ef108a7 sections=f99ce431 annotations=9be17165 navigation=a6fffdf2 links=efdc3cac pdf=96122dcc raw=0b45c8da',
  // wave 7c (W, item 4): its REF to the unterminated bm1 is Word's bold error text now (was f48e4c99/…/7b595536)
  // WordCraft study (fields, C1b and B22): pages 2–5 — C1b on page 2, and the header's `PAGE \* upperLetter` (Word's
  // unknown-switch error) on every page from 2; bit 21 gives the old pages back (was 06bed89e/321c2b87/6ebaa664/14697a8d/
  // 2c7c1968/7b35354f/1900cff5)
  f8__7__10: 'pages=5 messages=335e3ea3 sections=ba9074e5 annotations=d5ee2e6d navigation=352b6253 links=d974a1ae pdf=5e0a6e8b raw=87f42761',
};

function session(key) {
  const env = withFonts(uo.newEnv());
  const doc = uo.open(fixture(key));
  const layout = doc.layout(env, OPTS);
  const sums = [];
  const n = layout.pageCount;
  const msgs = [];
  for (let i = 0; i < n; i++) {
    const m = layout.paint.encodePage(i).take().data;
    assert.equal(String.fromCharCode(...m.subarray(0, 4)), 'UORP');
    msgs.push(m);
  }
  sums.push(`pages=${n}`, `messages=${hex(concat(msgs))}`);
  // the page messages with the Write section (G8), from a fresh layout (a fresh resource mark)
  const l2 = doc.layout(env, OPTS);
  const secs = [];
  for (let i = 0; i < n; i++) secs.push(l2.encodePage(i));
  assert.throws(() => l2.encodePage(n), /status 6/);
  l2.free();
  sums.push(`sections=${hex(concat(secs))}`, `annotations=${hex(encodeAnnotations(layout.annotations()))}`);
  // the decoded forms re-encoded give the raw bytes the native test pins
  sums.push(`navigation=${hex(rawLayout(layout, 'nav'))}`, `links=${hex(rawLayout(layout, 'links'))}`);
  const nav = layout.navigation();
  assert.ok(Array.isArray(nav.links) && Array.isArray(nav.outline));
  const info = layout.pageInfo(0);
  assert.ok(info.w > 0 && info.h > 0);
  assert.ok(layout.faceBytes(0)?.length > 1000);
  const pdf = layout.pdf();
  const all = concat(pdf.chunks);
  assert.equal(pdf.length, all.length);
  assert.equal(String.fromCharCode(...all.subarray(0, 5)), '%PDF-');
  sums.push(`pdf=${hex(all)}`);
  sums.push(`raw=${hex(concat(layout.pdf({ compress: false, chunk: 4096 }).chunks))}`);
  layout.free(); doc.free(); env.free();
  return sums.join(' ');
}

function rawLayout(layout, what) {
  return what === 'nav' ? encodeNav(layout.navigation()) : encodeBoxes(layout.runBoxes('links'));
}
// the inverse of the decoders (the layouts of src/write.rs), so the pinned native checksums apply
function encodeStr(parts, s) { const u = new Uint8Array(4 + s.length * 2); const d = new DataView(u.buffer); d.setUint32(0, s.length, true); for (let i = 0; i < s.length; i++) d.setUint16(4 + 2 * i, s.charCodeAt(i), true); parts.push(u); }
function u32(parts, v) { const u = new Uint8Array(4); new DataView(u.buffer).setUint32(0, v, true); parts.push(u); }
function f64(parts, v) { const u = new Uint8Array(8); new DataView(u.buffer).setFloat64(0, v, true); parts.push(u); }
function encodeNav(nav) {
  const p = [];
  u32(p, nav.links.length);
  for (const l of nav.links) {
    u32(p, l.page); f64(p, l.x); f64(p, l.y); f64(p, l.w); f64(p, l.h);
    if (l.uri !== undefined) { p.push(new Uint8Array([0])); encodeStr(p, l.uri); } else { p.push(new Uint8Array([1])); u32(p, l.dest.page); f64(p, l.dest.top); }
  }
  u32(p, nav.outline.length);
  for (const e of nav.outline) { u32(p, e.level); u32(p, e.page); f64(p, e.top); encodeStr(p, e.title); }
  return concat(p);
}
function encodeAnnotations(a) {
  const p = [];
  u32(p, a.comments.length);
  for (const c of a.comments) {
    f64(p, c.id);
    if (c.author !== undefined) { p.push(new Uint8Array([1])); encodeStr(p, c.author); } else p.push(new Uint8Array([0]));
    encodeStr(p, c.text);
    u32(p, c.rects.length);
    for (const r of c.rects) { u32(p, r.page); f64(p, r.x); f64(p, r.y); f64(p, r.w); f64(p, r.h); }
  }
  u32(p, a.headings.length);
  for (const h of a.headings) { f64(p, h.level); u32(p, h.page); f64(p, h.y); encodeStr(p, h.text); }
  return concat(p);
}
function encodeBoxes(boxes) {
  const p = [];
  u32(p, boxes.length);
  for (const b of boxes) { u32(p, b.page); f64(p, b.x); f64(p, b.y); f64(p, b.w); f64(p, b.h); f64(p, b.baseline); encodeStr(p, b.value); }
  return concat(p);
}

test('a viewer session gives the native bytes (pages, navigation, links, PDF)', { skip }, () => {
  for (const [key, want] of Object.entries(PINNED)) assert.equal(session(key), want, key);
});

test('pageHeads gives the size and background of every page, as the eager decode of each page does', { skip }, () => {
  for (const key of Object.keys(PINNED)) {
    const env = withFonts(uo.newEnv());
    const doc = uo.open(fixture(key));
    const layout = doc.layout(env, OPTS);
    const heads = layout.pageHeads();
    assert.equal(heads.length, layout.pageCount, key);
    const tables = newTables();
    for (let i = 0; i < layout.pageCount; i++) {
      const p = decodePage(layout.encodePage(i), tables);
      const want = { w: p.widthTw, h: p.heightTw, ...(p.background !== undefined ? { background: p.background } : {}) };
      assert.deepEqual(heads[i], want, `${key} page ${i}`);
    }
    layout.free();
    assert.throws(() => layout.pageHeads());
    doc.free(); env.free();
  }
});

test('pages decoded out of order (reverse, shuffled) equal the in-order decode, with one set of tables', { skip }, () => {
  const decodeAll = (layout, order) => {
    const tables = newTables();
    const wt = newWriteTables();
    const out = [];
    for (const i of order) {
      const p = decodePage(layout.encodePage(i), tables);
      const sec = decodeWritePage(p.app, wt);
      out[i] = { w: p.widthTw, h: p.heightTw, items: p.items, lines: sec.lines, paras: sec.paras };
    }
    return out;
  };
  for (const key of Object.keys(PINNED)) {
    const env = withFonts(uo.newEnv());
    const doc = uo.open(fixture(key));
    const n = doc.layout(env, OPTS);
    const count = n.pageCount;
    n.free();
    const inOrder = [...Array(count).keys()];
    const reverse = [...inOrder].reverse();
    const shuffled = [...inOrder.filter((i) => i % 2 === 1), ...inOrder.filter((i) => i % 2 === 0)].reverse();
    const run = (order) => { const l = doc.layout(env, OPTS); try { return decodeAll(l, order); } finally { l.free(); } };
    const want = run(inOrder);
    assert.deepEqual(run(reverse), want, `${key} reverse`);
    assert.deepEqual(run(shuffled), want, `${key} shuffled`);
    doc.free(); env.free();
  }
});

test('the PDF arrives in chunks of the asked size; a null layout is refused', { skip }, () => {
  const env = withFonts(uo.newEnv());
  const doc = uo.open(fixture('f8__7__1'));
  const layout = doc.layout(env, OPTS);
  const one = layout.pdf();
  const many = layout.pdf({ chunk: 1024 });
  assert.equal(one.chunks.length, 1);
  assert.ok(many.chunks.length > 10);
  assert.ok(many.chunks.slice(0, -1).every((c) => c.length === 1024));
  assert.deepEqual(concat(many.chunks), concat(one.chunks));
  assert.equal(w.uo_layout_pdf(0, 1, 0), -1);
  w.uo_last_error_clear();
  layout.free(); doc.free(); env.free();
});

test('errors carry a code, a stage and a message', { skip }, () => {
  assert.throws(() => uo.open(new Uint8Array([1, 2, 3])), (e) => e instanceof WriteError && e.code === 3 && e.stage === 'read' && e.detail.length > 0);
  const env = uo.newEnv();
  const doc = uo.open(fixture('f8__7__0'));
  assert.throws(() => doc.layout(env, OPTS), (e) => e instanceof WriteError && e.code === 11 && e.stage === 'layout');
  assert.throws(() => doc.layout(env, { view: 'nope' }), (e) => e instanceof WriteError && e.code === 1);
  assert.throws(() => env.pushAsset('X', 'regular', new Uint8Array([1, 2, 3, 4, 5])), (e) => e instanceof WriteError && e.code === 3);
  assert.throws(() => env.pushFamily('X', { faces: [{ variant: 'nope', bytes: new Uint8Array([1]) }] }), (e) => e instanceof WriteError && e.code === 1);
  doc.free(); env.free();
  assert.throws(() => doc.layout(env), /freed/);
});

// ---- the memory gate -------------------------------------------------------------------------------------------

/** the smallest WordprocessingML package (stored zip, built here) */
function tinyDocx() {
  const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = (b) => { let c = 0xffffffff; for (const x of b) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const files = [
    ['[Content_Types].xml', '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>'],
    ['_rels/.rels', '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>'],
    ['word/document.xml', '<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:rFonts w:ascii="Liberation Sans"/></w:rPr><w:t>Hello</w:t></w:r></w:p></w:body></w:document>'],
  ];
  const te = new TextEncoder();
  const parts = [], central = [];
  let off = 0;
  for (const [name, text] of files) {
    const n = te.encode(name), data = te.encode(text), c = crc(data);
    const h = new DataView(new ArrayBuffer(30));
    h.setUint32(0, 0x04034b50, true); h.setUint16(4, 20, true); h.setUint32(14, c, true); h.setUint32(18, data.length, true); h.setUint32(22, data.length, true); h.setUint16(26, n.length, true);
    const ch = new DataView(new ArrayBuffer(46));
    ch.setUint32(0, 0x02014b50, true); ch.setUint16(4, 20, true); ch.setUint16(6, 20, true); ch.setUint32(16, c, true); ch.setUint32(20, data.length, true); ch.setUint32(24, data.length, true); ch.setUint16(28, n.length, true); ch.setUint32(42, off, true);
    parts.push(new Uint8Array(h.buffer), n, data);
    central.push(new Uint8Array(ch.buffer), n);
    off += 30 + n.length + data.length;
  }
  const cd = concat(central);
  const end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, 0x06054b50, true); end.setUint16(8, files.length, true); end.setUint16(10, files.length, true); end.setUint32(12, cd.length, true); end.setUint32(16, off, true);
  return concat([...parts, cd, new Uint8Array(end.buffer)]);
}

const mskip = skip || (!counting && 'build with --features count,write');

test(`free() releases every environment, document and layout (M4, ${M4} each)`, { skip: mskip }, async () => {
  const tiny = tinyDocx();
  const env = withFonts(uo.newEnv());
  { const d = uo.open(tiny); d.layout(env, OPTS).free(); d.free(); } // warm-up: faces parsed, slots
  const before = await settle();
  for (let i = 0; i < M4; i++) uo.newEnv().free();
  for (let i = 0; i < M4; i++) uo.open(tiny).free();
  const doc = uo.open(tiny);
  for (let i = 0; i < M4; i++) {
    const l = doc.layout(env, OPTS);
    if (i % 1000 === 0) { assert.equal(l.pageCount, 1); l.encodePage(0); }
    l.free();
  }
  doc.free();
  assert.equal(await settle(), before);
  env.free();
});

test(`the FinalizationRegistry releases handles that were not freed (M4, ${M4} each)`, { skip: mskip || (!globalThis.gc && 'needs node --expose-gc') }, async () => {
  const tiny = tinyDocx();
  const env = withFonts(uo.newEnv());
  { const d = uo.open(tiny); d.layout(env, OPTS).free(); d.free(); }
  const before = await settle();
  (() => {
    for (let i = 0; i < M4; i++) uo.newEnv();
    for (let i = 0; i < M4; i++) uo.open(tiny);
    const doc = uo.open(tiny);
    for (let i = 0; i < M4; i++) doc.layout(env, OPTS);
  })();
  assert.ok(live() > before, 'the abandoned handles are counted while alive');
  assert.equal(await settle(), before);
  env.free();
});

test(`repeated viewer sessions: live bytes flat, memory.buffer stops growing (M2, ${SOAK} rounds)`, { skip: mskip }, async () => {
  const sizes = [], lives = [];
  for (let i = 0; i < SOAK; i++) {
    session('f8__7__0');
    lives.push(await settle());
    sizes.push(w.memory.buffer.byteLength);
  }
  const warm = Math.min(3, SOAK - 1);
  assert.ok(lives.slice(warm).every((v) => v === lives[warm]), `live bytes: ${lives}`);
  assert.ok(sizes.slice(warm).every((v) => v === sizes[warm]), `memory.buffer: ${sizes}`);
  console.log(`M2: ${SOAK} sessions, live ${lives[warm]} B after each, memory.buffer ${sizes[warm]} B`);
});
