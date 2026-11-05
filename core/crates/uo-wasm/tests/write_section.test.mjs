// G8 (uo-docx-layout record "Write page section (G8)"), Node only (it imports the generated TypeScript
// codecs, which a browser cannot): every page message of `layout.encodePage` for the committed fixture
// documents is decoded by the generated TS `render.ts` decoder, its `app` by the generated `write.ts`
// decoder; both re-encoded by the generated TS encoders give the SAME bytes, and the Write section
// arrives with the tree.ts names the shell's text layer reads (`lines[].ref/start/end/x/y/height/width/
// chars`, `paras[].text/level`).
//   cargo build -p uo-wasm --features count,write --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> node --test crates/uo-wasm/tests/write_section.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { writeImports, bindWrite } from '../js/write.mjs';
import * as R from '../js/gen/render.ts';
import * as W from '../js/gen/write.ts';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../..');
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const bytes = readFileSync(wasmPath);
const has = WebAssembly.Module.imports(new WebAssembly.Module(bytes)).some((i) => i.module === 'uo_host');
const skip = has ? false : 'this build has no write exports (--features write)';
const { imports, host } = writeImports();
const uo = has ? bindWrite((await WebAssembly.instantiate(bytes, imports)).instance.exports, host) : null;

const OPEN_FONTS = [
  ['Liberation Sans', 'site/assets/fonts', 'liberationsans'],
  ['Liberation Serif', 'shared/fonts/files', 'liberationserif'],
  ['Liberation Mono', 'shared/fonts/files', 'liberationmono'],
  ['Carlito', 'shared/fonts/files', 'carlito'],
  ['Caladea', 'shared/fonts/files', 'caladea'],
];
const VARIANTS = ['regular', 'bold', 'italic', 'boldItalic'];
const env = has ? uo.newEnv() : null;
if (has) {
  for (const [family, dir, stem] of OPEN_FONTS) {
    const faces = ['regular', 'bold', 'italic', 'bolditalic'].map((s, i) => ({ variant: VARIANTS[i], bytes: readFileSync(join(repo, dir, `${stem}-${s}.ttf`)) }));
    env.pushFamily(family, { source: 'installed', embeddable: true, faces });
  }
}
const OPTS = { generation: 1, now: 1_768_478_400_000, tzOffsetMin: 0, hyphenationLanguages: ['pt-BR', 'en', 'es'] };
const DOCS = ['f8__7__0', 'f8__7__1', 'f8__7__3', 'f8__7__5', 'f8__7__10'];
const fixture = (key) => readFileSync(join(repo, `core/crates/uo-docx-layout/tests/captures/f8/docs/${key}.docx`));

test('Rust encode -> TS decode -> TS encode gives the same bytes (render message and Write section, every page)', { skip }, () => {
  let pages = 0, sectionBytes = 0, renderBytes = 0;
  for (const key of DOCS) {
    const doc = uo.open(fixture(key));
    const layout = doc.layout(env, OPTS);
    const rt = R.newTables(), wt = W.newTables();
    for (let i = 0; i < layout.pageCount; i++) {
      const msg = layout.encodePage(i);
      const sent = R.tableLens(rt);
      const page = R.decodePage(msg, rt);
      assert.ok(page.app instanceof Uint8Array && page.app.length > 0, `${key} page ${i}: a Write section`);
      const back = R.encodePage(page, { generation: 1, tables: rt, sent });
      assert.deepEqual([...back], [...msg], `${key} page ${i}: the render message re-encodes`);
      const sec = W.decodeWritePage(page.app, wt);
      const again = W.encodeWritePage(sec, W.newEncodeState(1, wt));
      assert.deepEqual([...again], [...page.app], `${key} page ${i}: the Write section re-encodes`);
      sectionBytes += page.app.length;
      renderBytes += msg.length - page.app.length;
      pages++;
    }
    layout.free();
    doc.free();
  }
  console.log(`G8 R-P2b: ${pages} pages, Write sections ${sectionBytes} B, render parts ${renderBytes} B`);
});

test('the Write section has what the text layer reads, with the tree.ts names', { skip }, () => {
  const doc = uo.open(fixture('f8__7__1'));
  const layout = doc.layout(env, OPTS);
  const rt = R.newTables(), wt = W.newTables();
  let body = 0, withText = 0, headings = 0;
  for (let i = 0; i < layout.pageCount; i++) {
    const sec = W.decodeWritePage(R.decodePage(layout.encodePage(i), rt).app, wt);
    assert.equal(typeof sec.section, 'number');
    assert.equal(typeof sec.number, 'number');
    for (const l of sec.lines) {
      assert.equal(typeof l.ref, 'string');
      for (const k of ['start', 'end', 'x', 'y', 'height', 'baseline', 'width']) assert.equal(typeof l[k], 'number', k);
      if (l.chars) assert.ok(l.chars.every((c) => typeof c.o === 'number' && typeof c.x === 'number' && typeof c.w === 'number'));
      if (/^b\d/.test(l.ref)) body++;
      if (l.para !== undefined) {
        const p = sec.paras[l.para];
        assert.equal(typeof p.text, 'string');
        withText++;
        if (p.level !== undefined) headings++;
      } else assert.ok(!/^b\d/.test(l.ref) || !l.chars, `a body line with chars resolves: ${l.ref}`);
    }
  }
  assert.ok(withText > 0 && withText <= body, `${withText} of ${body} body lines carry their paragraph`);
  console.log(`text layer: ${withText} lines with a paragraph, ${headings} on headings`);
  layout.free();
  doc.free();
});
