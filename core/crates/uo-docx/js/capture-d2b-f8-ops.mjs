// Parity capture of D2b-F8's pure model files (record core/docs/modules/uo-docx-d2b/F8.md): the
// frozen editor's `paste-html.ts` pure exports (cssColor, cssSizeHalfPt, cssFamily, fragmentOf,
// wordListFormats, wordListOf, looksOrdered) over a seeded and hostile input set, and
// `numbering-store.ts` `DocNumbering.importAbstract` over the numbering parts of corpus documents
// (an abstractNum of another document imported into each; `Math.random` = the testkit xorshift32,
// seed 1 per document).
//
//   UO_TS_ROOT=<frozen built checkout> node --experimental-transform-types --no-warnings \
//     core/crates/uo-docx/js/capture-d2b-f8-ops.mjs [--docs N] [--cases N]
//
// Writes tests/parity/d2b-f8-ops.capture. Strings are hex UTF-16 units (`hx`). Keys `css#<j>`
// (`in`, `color`, `size`, `family`), `frag#<j>` (`in`, `out`), `wl#<j>` (`in`, `fmts`, `of`,
// `ord`), `<doc>#ia<j>` (`raw` = the source doc and abstract index, `id`, `xml`).
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { CaptureWriter, Rng, importFrom } from '../../uo-testkit/js/testkit.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 120);
const CASES = opt('--cases', 3000);

const { DocxPackage } = await ts('package.js');
const ph = await importFrom(root, 'write/packages/editor/src/paste-html.ts');
const nb = await importFrom(root, 'write/packages/editor/src/numbering-store.ts');

let rs = 1;
Math.random = () => { rs ^= rs << 13; rs >>>= 0; rs ^= rs >>> 17; rs ^= rs << 5; rs >>>= 0; return rs / 4294967296; };

export const hx = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
const sha = (t) => createHash('sha256').update(Buffer.from(hx(t))).digest('hex').slice(0, 32);
const val = (v) => (v === undefined ? 'u' : typeof v === 'number' ? `n${String(v)}` : `s${hx(v)}`);

const out = new CaptureWriter([
  'D2b-F8 pure model files: paste-html.ts (cssColor, cssSizeHalfPt, cssFamily, fragmentOf, wordListFormats, wordListOf, looksOrdered)',
  'and numbering-store.ts importAbstract over corpus numbering parts',
]);

const rng = new Rng(0xf8);
const pick = (a) => a[rng.below(a.length)];
const SP = [' ', '', '  ', '\t', '\n', ' ', ' ', '﻿', '\u0085', '\u000b'];
const NUMS = ['0', '1', '12', '255', '256', '999', '007', '1.5', '.5', '0.0', '1e3', '-1', '', '99999999999999999999', '000'];
const COLOR_WORDS = ['red', 'RED', 'Navy', 'grey', 'gray', 'fuchsia', 'aqua', 'transparent', 'inherit', 'initial', 'currentColor', 'currentcolor', 'none', 'rebeccapurple', 'black', 'white', 'lime', 'teal'];
const SIZE_WORDS = ['xx-small', 'x-small', 'small', 'medium', 'large', 'x-large', 'xx-large', 'XX-Large', 'smaller', 'larger', 'constructor', 'toString', '__proto__', 'valueOf', 'hasOwnProperty'];
const FAMS = ['Arial', '"Times New Roman"', "'Courier New'", 'serif', 'Sans-Serif', 'monospace', 'cursive', 'fantasy', 'system-ui', 'ui-monospace', 'ui-', 'ui-sans_serif2', '-apple-system',
  'BlinkMacSystemFont', 'inherit', 'initial', 'Calibri', '"', "'", '""', '"Aptos', 'Aptos"', 'Segoe UI', '  ', 'Serif', 'ſerif', 'Cambria Math', '😀 Font', '\uD83D'];

function colorCase() {
  const k = rng.below(10);
  const w = () => pick(SP);
  switch (k) {
    case 0: return `#${[...Array(3)].map(() => pick('0123456789abcdefABCDEFgz')).join('')}`;
    case 1: return `#${[...Array(rng.below(9))].map(() => pick('0123456789abcdefABCDEF')).join('')}`;
    case 2: case 3: {
      const a = rng.below(2) ? 'a' : '';
      const sep = () => pick([',', ', ', ' ', ' , ', ',,', '\t']);
      const alpha = rng.below(3) === 0 ? '' : `${pick([',', ' /', '/', ', '])}${pick(['0', '0.0', '0%', '.5', '1', '50%', '0.', '..', '0.00', '1.0'])}`;
      return `${w()}${pick(['rgb', 'RGB', 'rgba', 'Rgb'])}${a}(${w()}${pick(NUMS)}${sep()}${pick(NUMS)}${sep()}${pick(NUMS)}${alpha}${w()})${w()}`;
    }
    case 4: return `${w()}${pick(COLOR_WORDS)}${w()}`;
    case 5: return pick(['hsl(0, 100%, 50%)', 'rgb(1,2)', 'rgb(1,2,3,4,5)', 'rgb(1 2 3 / 0)', 'rgb( 1 , 2 , 3 )', 'rgba(255,255,255,0.000)', '#fff ', ' #FFFFFF00', '#12345', 'rgb(1,2,3)x', 'rgb(-1,2,3)']);
    default: return [...Array(rng.below(12))].map(() => pick(['r', 'g', 'b', 'a', '(', ')', ',', ' ', '1', '#', 'f', '/', '%', '.', 'ſ', '\uD800'])).join('');
  }
}

function sizeCase() {
  const k = rng.below(6);
  if (k === 0) return `${pick(SP)}${pick(SIZE_WORDS)}${pick(SP)}`;
  if (k < 4) return `${pick(SP)}${pick(['12', '10.5', '0', '0.1', '.25', '1639.9', '1640', '999', '1e3', '72', '0.75', '..', '1.2.3', '-5', '+5'])}${pick(['pt', 'px', 'PT', 'Px', 'em', '%', '', 'pt '])}${pick(SP)}`;
  return [...Array(rng.below(8))].map(() => pick(['1', '.', 'p', 't', 'x', ' ', 'e', '-'])).join('');
}

function familyCase() {
  return [...Array(1 + rng.below(4))].map(() => `${pick(SP)}${pick(FAMS)}${pick(SP)}`).join(pick([',', ', ', ' ,']));
}

for (let j = 0; j < CASES; j++) {
  const key = `css#${j}`;
  const c = colorCase(), s = sizeCase(), f = familyCase();
  out.set(key, 'in', `${hx(c)}|${hx(s)}|${hx(f)}`);
  out.set(key, 'color', val(ph.cssColor(c)));
  out.set(key, 'size', val(ph.cssSizeHalfPt(s)));
  out.set(key, 'family', val(ph.cssFamily(f)));
}

const FRAG = ['<!--StartFragment-->', '<!--EndFragment-->', 'x', '<p>a</p>', '<!--StartFragment', '\r\n', 'é', '😀'];
for (let j = 0; j < 300; j++) {
  const t = [...Array(rng.below(8))].map(() => pick(FRAG)).join('');
  out.set(`frag#${j}`, 'in', hx(t));
  out.set(`frag#${j}`, 'out', hx(ph.fragmentOf(t)));
}

const LIST = ['@list l0:level1 { mso-level-number-format:bullet; }', '@list l0:level2\n{mso-level-number-format: image}', '@LIST L1:LEVEL1{mso-level-text:"%1."}', '@list l01:level01 {mso-level-number-format:roman-lower;}',
  '@list  l2:level3 {MSO-LEVEL-NUMBER-FORMAT:Bullet}', '@list l3:level1', '{}', '@list l3:level1 {', '}', '@list l4:level9 {mso-level-number-format:}', '@list l4:level9 {mso-level-number-format: -x_1}',
  '@listl5:level1{}', '@list l6:levelx{}', 'mso-level-number-format:bullet', ' ', ' ', '@list l7:level1 {mso-level-number-format:bullet}'];
const STYLE = ['mso-list:l0 level1 lfo1', 'MSO-LIST: L1 LEVEL2 LFO3', 'mso-list:l0 level1', 'margin:0;mso-list:l12 level3 lfo007', 'mso-list:\tl0 level1 lfo2', 'mso-list:Ignore', 'mso-list: ignore',
  'mso-list:l0  level99 lfo1', 'mso-list:l99999999999999999999 level1 lfo1', 'mso-list:l0 level1 lfo', 'x mso-list:l2 level1 lfo1 mso-list:l3 level2 lfo2', ''];
const MARK = ['1.', '(1)', '1)', 'a)', 'iv.', '(iv)', 'abcd.', 'abcde.', '·', 'o', '§', ' 1 . ', 'A.', '12', '(a', 'a', '', '1.)', ' ', 'ſ.', '(ii)'];
for (let j = 0; j < 600; j++) {
  const html = [...Array(rng.below(6))].map(() => pick(LIST)).join(pick(['', ' ', '\n']));
  const style = pick(STYLE);
  const mark = [...Array(1 + rng.below(2))].map(() => pick(MARK)).join(pick(['', ' ', '\t']));
  const key = `wl#${j}`;
  out.set(key, 'in', `${hx(html)}|${hx(style)}|${hx(mark)}`);
  out.set(key, 'fmts', [...ph.wordListFormats(html)].map(([k, v]) => `${k}=${v ? 1 : 0}`).join(' ') || '-');
  const of = ph.wordListOf(j % 7 === 0 ? null : style);
  out.set(key, 'of', of ? `${String(of.list)},${String(of.level)},${String(of.lfo)}` : '-');
  out.set(key, 'ord', ph.looksOrdered(mark) ? '1' : '0');
}

// ── importAbstract ──
const docs = [];
for (const { rel, file } of corpus().filter((_, i) => i % 5 === 1)) {
  if (docs.length >= DOCS) break;
  let pkg;
  try { pkg = DocxPackage.load(readBytes(file)); } catch { continue; }
  const x = pkg.getPartText('word/numbering.xml');
  docs.push({ rel, xml: x ?? null, abs: x ? [...x.matchAll(/<w:abstractNum\b[\s\S]*?<\/w:abstractNum>/g)].map((m) => m[0]) : [] });
}
const withAbs = docs.filter((d) => d.abs.length);
for (const [di, d] of docs.entries()) {
  rs = 1;
  const num = new nb.DocNumbering(d.xml);
  for (let j = 0; j < 4 && withAbs.length; j++) {
    const si = rng.below(withAbs.length);
    const ai = rng.below(withAbs[si].abs.length);
    const raw = withAbs[si].abs[ai];
    const key = `${d.rel}#ia${j}`;
    out.set(key, 'doc', String(di));
    out.set(key, 'raw', `${withAbs[si].rel}|${ai}`);
    const id = num.importAbstract(raw);
    out.set(key, 'id', String(id));
    out.set(key, 'xml', sha(num.xml ?? ''));
  }
}
out.write(join(crate, 'tests', 'parity', 'd2b-f8-ops.capture'));
console.log(`d2b-f8-ops: ${CASES} css cases, ${docs.length} documents`);
