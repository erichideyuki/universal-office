// Captures the frozen TS over the corpus of tests/parity/frontier_d_inputs.txt (written by
// gen_frontier_d_inputs.mjs) and writes tests/parity/frontier_d.cap, replayed by
// tests/frontier_d_parity.rs:
//   odttf    obfuscateOdttf (shared/fonts ooxml.ts) and deobfuscateFont (docx-core fonts.ts)
//   style    variantOfStyle (shared/fonts types.ts)
//   lang     hyphenationPatternId (docx-layout hyphen/index.ts)
//   hyphen   Hyphenator.points and hyphenate(word, l, r) for l, r in 1..=3 over the words of each
//            language, as digests of 250-word chunks (the Hyphenator is built from the shipped
//            pattern module, as the layout does)
//   catalog  the bundled/substitute lookups, in the TS order
//
//   UO_TS_ROOT=<built checkout of main> node tests/parity_support/capture_frontier_d.mjs
import { execSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Stream, importFrom, tsRoot } from '../../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = tsRoot();
const types = await importFrom(root, 'shared/fonts/dist/types.js');
const ooxml = await importFrom(root, 'shared/fonts/dist/ooxml.js');
const docxFonts = await importFrom(root, 'write/packages/docx-core/dist/fonts.js');
const hyphenIndex = await importFrom(root, 'write/packages/docx-layout/dist/typography/hyphen/index.js');
const { Hyphenator } = await importFrom(root, 'write/packages/docx-layout/dist/typography/hyphen/liang.js');

const sha = (u8) => createHash('sha256').update(u8).digest('hex').slice(0, 32);
const fromUnits = (field) => (field === '~' ? undefined : String.fromCharCode(...field.split(',').filter(Boolean).map((h) => parseInt(h, 16))));
const lines = readFileSync(join(here, '..', 'parity', 'frontier_d_inputs.txt'), 'utf8').split('\n').filter(Boolean).map((l) => l.split('\t'));

const cap = new CaptureWriter(['shared/fonts + docx-core + docx-layout small facets of uo-fonts frontier D; digests and short values only']);
cap.meta('ts-source', 'shared/fonts/src/{types,ooxml}.ts write/packages/docx-core/src/fonts.ts write/packages/docx-layout/src/typography/hyphen');
cap.meta('ts-commit', execSync('git rev-parse --short=8 HEAD', { cwd: root }).toString().trim());

const data = new Uint8Array(100).map((_, i) => (i * 7 + 3) & 0xff);
let n = 0;
for (const [kind, a, b] of lines) {
  if (kind === 'odttf') {
    const key = fromUnits(a);
    const k = `odttf/${n++}`;
    cap.set(k, 'in', a);
    cap.set(k, 'ob', sha(ooxml.obfuscateOdttf(data, key)));
    const de = docxFonts.deobfuscateFont(data, key);
    cap.set(k, 'de', de === data ? 'same' : sha(de));
  }
}
n = 0;
for (const [kind, a] of lines) {
  if (kind === 'style') {
    const k = `style/${n++}`;
    cap.set(k, 'in', a);
    cap.set(k, 'variant', String(types.variantOfStyle(fromUnits(a))));
  }
}
n = 0;
for (const [kind, a] of lines) {
  if (kind === 'lang') {
    const k = `lang/${n++}`;
    cap.set(k, 'in', a);
    cap.set(k, 'id', String(hyphenIndex.hyphenationPatternId(fromUnits(a))));
  }
}

const IDS = hyphenIndex.HYPHENATION_PATTERN_IDS;
cap.set('hyphen', 'ids', IDS.join(','));
for (const id of IDS) {
  const { default: patterns } = await importFrom(root, `write/packages/docx-layout/dist/typography/hyphen/patterns/${id}.js`);
  const h = new Hyphenator(patterns);
  const words = lines.filter((l) => l[0] === 'word' && l[1] === id).map((l) => l[2]);
  cap.set(`hyphen/${id}`, 'words', String(words.length));
  cap.set(`hyphen/${id}`, 'minimums', `${patterns.left},${patterns.right}`);
  for (let from = 0, chunk = 0; from < words.length; from += 250, chunk++) {
    const st = new Stream();
    for (const w of words.slice(from, from + 250)) {
      st.tag('w');
      const pts = h.points(w);
      st.u32(pts.length);
      for (const p of pts) st.u32(p);
      for (let l = 1; l <= 3; l++) for (let r = 1; r <= 3; r++) {
        const hy = h.hyphenate(w, l, r);
        st.u32(hy.length);
        for (const p of hy) st.u32(p);
      }
    }
    cap.set(`hyphen/${id}`, `chunk${chunk}`, st.finish().short);
  }
}

const catalog = await importFrom(root, 'shared/fonts/dist/catalog.js');
const names = ['calibri', 'cambria', 'arial', 'arial mt', 'arial unicode ms', 'helvetica', 'times', 'tinos', 'courier', 'cousine', 'carlito', 'caladea', 'liberation sans', 'liberation mono', 'verdana', 'Calibri', ''];
for (const f of names) {
  cap.set(`catalog/${f}`, 'substitute', String(Object.hasOwn(catalog.METRIC_SUBSTITUTES, f) ? catalog.METRIC_SUBSTITUTES[f] : null));
  cap.set(`catalog/${f}`, 'bundled', Object.hasOwn(catalog.BUNDLED, f) ? JSON.stringify(catalog.BUNDLED[f]) : 'null');
}
for (const v of types.VARIANTS) cap.set(`catalog/file/${v}`, 'name', catalog.bundledFileName('carlito', v));

cap.write(join(here, '..', 'parity', 'frontier_d.cap'));
console.log(`frontier_d.cap: ${cap.records.size} records`);
