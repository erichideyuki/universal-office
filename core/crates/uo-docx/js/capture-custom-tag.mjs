// Parity capture of the custom-inline tag parser (frontier R4c: `JSON.parse` and
// `decodeURIComponent` of `parseCustomInlineTag`, document.ts:1760). V8 against
// `read::json_parse` + `read::paragraph::parse_custom_inline_tag`:
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-custom-tag.mjs
//
// The inputs are the committed `tests/parity/custom-tag.inputs.txt` (written here, seeded: hand-
// written tags, generated JSON of every shape (numbers, strings with escapes and surrogates, nested
// arrays and objects with duplicate and integer-like keys, white space) percent-encoded in
// several ways, and mutations of them: a deleted, inserted or swapped character, a truncation).
// One record per input line number with the facet `tag`: `undef` (the TS returns `undefined`)
// or the digest of the canonical stream of `{ type, data }` (js/canon.mjs). The function below is
// the TS `parseCustomInlineTag` as written in the frozen source.
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Stream, Rng } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const PREFIX = 'wordnext:inline:';

/** document.ts `parseCustomInlineTag`, verbatim. */
function parseCustomInlineTag(tag) {
  if (!tag?.startsWith(PREFIX)) return undefined;
  const payload = tag.slice(PREFIX.length);
  const split = payload.indexOf(':');
  if (split <= 0) return undefined;
  const type = payload.slice(0, split);
  if (!/^[a-zA-Z0-9._-]+$/.test(type)) return undefined;
  try {
    return { type, data: JSON.parse(decodeURIComponent(payload.slice(split + 1))) };
  } catch {
    return undefined;
  }
}

const rng = new Rng(0xc057);
const pick = (a) => a[rng.below(a.length)];
const chance = (n) => rng.below(n) === 0;
const NUMS = ['0', '-0', '1', '-1', '12', '1e5', '1E-2', '0.1', '-1.5e+3', '123456789012345678901234567890', '1e999', '-1e999', '0e0', '2.5', '1e-400', '9007199254740993', '0.30000000000000004', '5e-324', '1.7976931348623157e308', '1.7976931348623159e308'];
const STRS = ['', 'a', 'é', '😀', 'a\\nb', 'q\\"q', 'sl\\\\ash', 'sol\\/idus', '\\u00e9', '\\ud83d\\ude00', '\\ud800', '\\udc00', '\\ud800x', 'x\\udfff', '\\u0000', '\\b\\f\\r\\t', 'k:v', 'a,b', '[x]', '{y}', '%', ' ', ' ', 'long '.repeat(20)];
const KEYS = ['a', 'b', 'k', '0', '1', '2', '10', '4294967294', '4294967295', '01', '-1', '1.5', '__proto__', 'constructor', 'é', 'a b', '', 'toString'];
const WS = ['', '', '', ' ', '\n', '\t', '\r\n', '  '];

function value(depth) {
  const k = rng.below(depth >= 3 ? 5 : 8);
  switch (k) {
    case 0: return pick(NUMS);
    case 1: return `"${pick(STRS)}"`;
    case 2: return pick(['true', 'false', 'null']);
    case 3: return pick(NUMS);
    case 4: return `"${pick(STRS)}"`;
    case 5:
    case 6: {
      const n = rng.below(5);
      return `[${Array.from({ length: n }, () => WS_(value(depth + 1))).join(',')}]`;
    }
    default: {
      const n = rng.below(5);
      return `{${Array.from({ length: n }, () => `${WS_(`"${pick(KEYS)}"`)}:${WS_(value(depth + 1))}`).join(',')}}`;
    }
  }
}
const WS_ = (s) => `${pick(WS)}${s}${pick(WS)}`;

function mutate(s) {
  if (!s.length) return s;
  const i = rng.below(s.length);
  switch (rng.below(6)) {
    case 0: return s.slice(0, i) + s.slice(i + 1);
    case 1: return s.slice(0, i) + pick([',', ']', '}', '[', '{', '"', ':', '\\', '\n', ' ', ' ', '+', '.', 'e', '0', '\u0001', "'", 'x', '﻿']) + s.slice(i);
    case 2: return s.slice(0, i);
    case 3: { const j = rng.below(s.length); const a = [...s]; [a[i], a[j]] = [a[j], a[i]]; return a.join(''); }
    case 4: return s + pick([' ', ',', 'x', '1', '\n', '}', ']']);
    default: return pick([' ', '\n']) + s;
  }
}

const encoders = [
  (j) => encodeURIComponent(j),
  (j) => encodeURIComponent(j),
  (j) => encodeURIComponent(j),
  (j) => j,
  (j) => encodeURI(j),
  (j) => encodeURIComponent(j).replace(/%7B/g, '{').replace(/%7D/g, '}').replace(/%22/g, '"'),
  (j) => encodeURIComponent(j) + pick(['%', '%2', '%zz', '%E2%82', '%C0%80', '%ED%A0%80', '%F4%90%80%80', '%80', '%F8%88%80%80%80']),
  (j) => pick(['%E2%82%AC', '%F0%9F%98%80', '%c3%a9', '%C3', '%00', '%25']) + encodeURIComponent(j),
  (j) => [...j].map((c) => (chance(3) ? encodeURIComponent(c) : c)).join(''),
  (j) => [...j].map((c) => (chance(2) ? `%${c.charCodeAt(0).toString(16).padStart(2, '0')}` : c)).filter((x) => x.length).join('').replace(/%([89a-f][0-9a-f])/g, '%$1'),
];
const TYPES = ['a', 'clickcompliance.document-tag', 'a.b-c_d9', 'T', 'a b', '', 'é', 'a/b', '-', '9', 'x'.repeat(40)];

const inputs = [
  '', 'wordnext:inline:', 'wordnext:inline::x', 'wordnext:inline:a', 'wordnext:inline:a:', 'wordnext:inline:a:1', 'wordnext:inline:a:%7B%7D', 'WORDNEXT:inline:a:1', 'wordnext:inline:a b:1',
  'wordnext:inline:a:{"a":1}', 'wordnext:inline:a:%E2%82%AC', 'wordnext:inline:a:"%E2%82%AC"', 'wordnext:inline:a:"%E2%82%"', 'wordnext:inline:a:1:2', 'wordnext:inline:é:1', 'x:wordnext:inline:a:1',
  'wordnext:inline:a:' + encodeURIComponent('{"tag":"name","label":"Nome é"}'), 'wordnext:inline:a:' + encodeURIComponent('[' .repeat(100) + ']'.repeat(100)),
  'wordnext:inline:a:' + encodeURIComponent('{"a":'.repeat(100) + '1' + '}'.repeat(100)), 'wordnext:inline:a:' + encodeURIComponent('['.repeat(128) + ']'.repeat(128)),
];
for (let i = 0; i < 3000; i++) {
  let json = WS_(value(0));
  if (chance(3)) json = mutate(json);
  if (chance(8)) json = mutate(json);
  json = json.toWellFormed();
  const enc = pick(encoders)(json);
  inputs.push(`${pick(['wordnext:inline:', 'wordnext:inline:', 'wordnext:inline:', 'wordnext:inline'])}${pick(TYPES)}:${enc}`);
}

const esc = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
const unesc = (x) => x.replace(/\\(.)/gs, (_, c) => (c === 'n' ? '\n' : c === 'r' ? '\r' : c === 't' ? '\t' : c));
const file = join(here, '..', 'tests', 'parity', 'custom-tag.inputs.txt');
writeFileSync(file, inputs.map((x) => esc(x.toWellFormed())).join('\n') + '\n');
const back = readFileSync(file, 'utf8').split('\n').slice(0, -1).map(unesc);
if (back.length !== inputs.length) throw new Error('input count changed on the round trip');

const cap = new CaptureWriter(['custom-inline tag facet of the D1a reader (frontier R4c): digests of the TS parseCustomInlineTag result, see js/capture-custom-tag.mjs']);
let some = 0;
back.forEach((tag, i) => {
  const r = parseCustomInlineTag(tag);
  if (r === undefined) return void cap.set(String(i), 'tag', 'undef');
  const s = new Stream();
  new Canon(s).value(r, 'custom');
  cap.set(String(i), 'tag', s.finish().short);
  some++;
});
cap.meta('corpus', `inputs=${inputs.length} parsed=${some}`);
cap.write(join(here, '..', 'tests', 'parity', 'custom-tag.capture'));
console.log(`${inputs.length} inputs, ${some} parsed`);
