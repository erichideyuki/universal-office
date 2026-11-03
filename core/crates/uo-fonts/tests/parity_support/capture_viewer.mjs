// Captures the `viewer` provider policy: the REAL `ViewerFonts` of write/packages/viewer/src/fonts.ts
// (dist) over a stub resolver and a stub `fetch`, driven by the scenarios of
// tests/parity/provider_scenarios.txt (viewer part). Writes tests/parity/provider_viewer.cap,
// replayed by tests/provider_parity.rs (`viewer_policy_matches_the_ts`).
//
//   cargo run --release -p uo-fonts --example provider_corpus -- <corpus folder>
//   UO_TS_ROOT=<built checkout of main> node tests/parity_support/capture_viewer.mjs --corpus <corpus folder> [--trace <file>]
//
// Per scenario (key `viewer/<id>`): the stream of the answers (`q`: one line per ask) and of the
// `missing` notes (`m`), as `items:digest`. The line of an answer is
//   key|family|sha256|bold italic synthBold synthItalic standIn|identity|adv=<advance of "a" at 11 pt, from the table, or ->
// where identity is the index of the first answer of the scenario that is the same object.
import { execSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Stream, importFrom, tsRoot } from '../../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = tsRoot();
const argv = process.argv.slice(2);
const traceFile = argv.includes('--trace') ? argv[argv.indexOf('--trace') + 1] : null;

const { ViewerFonts } = await importFrom(root, 'write/packages/viewer/dist/fonts.js');
const sha = (u8) => createHash('sha256').update(u8).digest('hex');

const corpus = argv.includes('--corpus') ? argv[argv.indexOf('--corpus') + 1] : null;
if (!corpus) throw new Error('--corpus <folder written by: cargo run -p uo-fonts --example provider_corpus -- <folder>>');
const POOL = {};
for (const name of ['carlito-regular', 'carlito-bold', 'carlito-italic', 'carlito-bolditalic', 'caladea-regular', 'caladea-bold', 'serif-regular', 'mono-regular', 'garbage', 'cut']) {
  POOL[name] = new Uint8Array(readFileSync(join(corpus, 'pool', `${name}.bin`)));
}
const VARIANT = { r: 'regular', b: 'bold', i: 'italic', z: 'boldItalic' };
// the asset files of viewer fonts.ts ASSET_FILES, by (family, variant)
const ASSET_FILE = {
  'Liberation Sans|regular': 'liberationsans-regular.ttf', 'Liberation Sans|bold': 'liberationsans-bold.ttf',
  'Liberation Sans|italic': 'liberationsans-italic.ttf', 'Liberation Sans|boldItalic': 'liberationsans-bolditalic.ttf',
  'Aptos|regular': 'aptos-regular.ttf', 'Aptos|bold': 'aptos-bold.ttf', 'Aptos|italic': 'aptos-italic.ttf', 'Aptos|boldItalic': 'aptos-bolditalic.ttf',
  'Calibri|regular': 'calibri.ttf', 'Calibri|bold': 'calibrib.ttf', 'Calibri|italic': 'calibrii.ttf', 'Calibri|boldItalic': 'calibriz.ttf',
};
const ADVANCES = { 11: { a: 123.5, b: 7 }, 12: { a: 9 } };

const lines = readFileSync(join(here, '..', 'parity', 'provider_scenarios.txt'), 'utf8').split('\n').filter(Boolean).map((l) => l.split('\t'));
const scenarios = [];
for (let i = 0; i < lines.length; i++) {
  if (lines[i][0] !== 'V') continue;
  const ops = [];
  let j = i + 1;
  while (lines[j][0] !== 'E') ops.push(lines[j++]);
  scenarios.push({ id: lines[i][1], ops });
  i = j;
}

const cap = new CaptureWriter(['the `viewer` provider policy: ViewerFonts of write/packages/viewer over a stub resolver; digests only']);
cap.meta('ts-source', 'write/packages/viewer/src/fonts.ts');
cap.meta('ts-commit', execSync('git rev-parse --short=8 HEAD', { cwd: root }).toString().trim());
cap.meta('inputs', `${scenarios.length} scenarios`);
const trace = [];

for (const sc of scenarios) {
  const table = new Map();
  const loads = [];
  const resolver = {
    suggestBaseUrl() {},
    getWithFile: (family) => table.get(family.toLowerCase()),
    resolveWithFile: async (families) => [...families].map((family) => ({ family, faces: {} })),
    loadFace: async (r, v) => { loads.push(`${r.family}:${v}`); },
  };
  const assets = new Map();
  const advances = new Set();
  for (const [op, family, variant, pool, adv] of sc.ops) {
    if (op !== 'a') continue;
    const file = ASSET_FILE[`${family}|${VARIANT[variant]}`];
    assets.set(file, POOL[pool]);
    if (adv === '1') advances.add(file.replace(/\.ttf$/, ''));
  }
  const names = new Map(Object.entries(ASSET_FILE).map(([k, file]) => {
    const [family, v] = k.split('|');
    const prefix = family.toLowerCase().replace(/\s+/g, '');
    const suffix = v === 'regular' ? '' : v === 'boldItalic' ? '-bolditalic' : `-${v}`;
    return [`${prefix}${suffix}-advances.json`, file];
  }));
  globalThis.fetch = async (url) => {
    const name = url.slice(url.lastIndexOf('/') + 1);
    if (assets.has(name)) {
      const b = assets.get(name);
      return { ok: true, arrayBuffer: async () => b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength) };
    }
    const asset = names.get(name);
    if (asset && advances.has(asset.replace(/\.ttf$/, '')) && assets.has(asset)) return { ok: true, json: async () => ADVANCES };
    return { ok: false };
  };
  const vf = new ViewerFonts(resolver, () => {});
  await vf.loadAssets('https://x/fonts');

  const s = new Stream();
  const seen = [];
  let provider = vf.provider();
  const out = [];
  for (const op of sc.ops) {
    if (op[0] === 'f') {
      const [, family, source, embeddable, faces] = op;
      const map = {};
      for (const pair of faces.split(',')) { const [v, pool] = pair.split('='); map[VARIANT[v]] = POOL[pool]; }
      table.set(family.toLowerCase(), { family, cssFamily: family, source, faces: map, embeddable: embeddable === '1' });
    } else if (op[0] === 'c') {
      provider = vf.provider();
    } else if (op[0] === 'q') {
      const [, family, b, i] = op;
      const a = provider.face(family, b === '1', i === '1');
      let line = '-';
      if (a) {
        let at = seen.indexOf(a);
        if (at < 0) { seen.push(a); at = seen.length - 1; }
        const f = (x) => (x ? 1 : 0);
        const adv = a.font.advanceTable?.['11']?.a;
        line = `${a.key}|${a.family}|${sha(a.bytes)}|${f(a.bold)}${f(a.italic)}${f(a.synthBold)}${f(a.synthItalic)}${f(a.standIn)}|${at}|adv=${adv === undefined ? '-' : adv}`;
      }
      s.tag('q').str(line);
      out.push(`q ${family} ${b}${i} ${line}`);
    } else if (op[0] === 'm') {
      const dump = [...vf.missing].map(([f, set]) => `${f}:${[...set].join('+')}`).join(',');
      loads.length = 0;
      const did = await vf.loadMissing();
      const line = `${dump}|${did ? 1 : 0}|${loads.join(';')}`;
      s.tag('m').str(line);
      out.push(`m ${line}`);
    }
  }
  const d = s.finish();
  cap.set(`viewer/${sc.id}`, 'stream', `${s.items}:${d.short}`);
  trace.push(`## ${sc.id}`, ...out);
}

cap.write(join(here, '..', 'parity', 'provider_viewer.cap'));
if (traceFile) writeFileSync(traceFile, trace.join('\n') + '\n');
console.log(`provider_viewer.cap: ${scenarios.length} scenarios`);
