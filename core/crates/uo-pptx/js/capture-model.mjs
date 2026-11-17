// Parity capture of the Slides read path (plan section 4.3, B1): the presentation and slide models
// the frozen TS reader builds (`readPresentation`, `readSlide`, `readSlideNotes` of slides-core,
// over `PptxPackage.load`, which converts a binary .ppt in memory first) as canonical JSON
// (lib/canon-json.mjs), one record per deck, in the uo-testkit capture format.
//
//   node core/crates/uo-pptx/js/capture-model.mjs --samples                 # the tracked samples
//   node core/crates/uo-pptx/js/capture-model.mjs --gen b1a                 # a generated group, in memory
//   node --max-old-space-size=8192 core/crates/uo-pptx/js/capture-model.mjs --gen b1-big   # local only
//   options: --out <file>  --detail <file.json.gz> (the canonical JSON text itself, per deck)
//            --decks <dir> (read the generated decks from disk instead of regenerating them)
//            --only <substring of the key>  --ts-root <dir> (default UO_TS_ROOT, else the 0.9.2 reference)
//
// Records (key `samples/<path>` or `gen/<group>/<NNNN>`), facets, each `len:sha256[0..32]` of the
// UTF-8 canonical JSON unless said otherwise:
//   input   `len:sha256[0..16]` of the deck bytes (the .ppt itself for legacy samples)
//   seed    (generated) the seed of the deck
//   read    `ok`, or `err:<one-line message>` when the package or presentation could not be read
//   pres    the Presentation (slide size, size type, slide refs, show settings)
//   slides  number of slides
//   s<N>    slide N (0-based): `{"notes":<text>,"slide":<Slide>}`; `err:<message>` when readSlide throws
//   model   SHA-256[0..32] over pres + each slide text joined by newlines (a one-facet check)
//   n<N>    slide N WITHOUT `timing` (`{"notes":…,"slide":<Slide minus timing>}`): the model the Rust read path
//           (frontier B1b) compares before `parseSlideTiming` is ported (B6); `nmodel` is `model` over pres + n<N>
//   dropped what the canonical JSON dropped or digested in this deck (`what:count,...`; the policy
//           is in canon-json.mjs and in the capture header)
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { gzipSync } from 'node:zlib';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { LONG_TEXT, bytesDigest, canonJson, textDigest } from './lib/canon-json.mjs';
import { args, capturesDir, loadSlidesCore, oneLine, readSample, trackedSamples, tsCommit, tsRoot } from './lib/common.mjs';
import { GROUPS, buildGroupDeck, deckFile, groupKey } from './lib/gen/groups.mjs';

const a = args();
const root = tsRoot(a);
const core = await loadSlidesCore(root);
const only = a.opt('--only');
const detailPath = a.opt('--detail');

/** The model of one deck as canonical JSON texts: { pres, slides: [text | {err}], dropped } or { err }. */
export function modelTexts(bytes) {
  const dropped = new Map();
  let pkg, pres;
  try {
    pkg = core.PptxPackage.load(bytes);
    pres = core.readPresentation(pkg);
  } catch (e) {
    return { err: oneLine(e?.message ?? e) };
  }
  const presText = canonJson(pres, dropped);
  const nts = [];
  const slides = pres.slides.map((ref) => {
    if (ref.target === undefined) { nts.push(null); return { err: 'no-target' }; }
    try {
      const slide = core.readSlide(pkg, ref.target);
      let notes = '';
      try { notes = core.readSlideNotes(pkg, ref.target); } catch (e) { notes = `\u0000error:${oneLine(e?.message ?? e)}`; }
      const text = canonJson({ notes, slide }, dropped);
      // the same without `timing` (undefined keys are omitted by the canon): a separate drop count
      nts.push(canonJson({ notes, slide: { ...slide, timing: undefined } }, new Map()));
      return text;
    } catch (e) {
      nts.push(null);
      return { err: oneLine(e?.message ?? e) };
    }
  });
  return { pres: presText, slides, nts, dropped };
}

function record(w, key, bytes, extra = {}) {
  w.set(key, 'input', bytesDigest(bytes));
  for (const [k, v] of Object.entries(extra)) w.set(key, k, v);
  const m = modelTexts(bytes);
  if (m.err !== undefined) {
    w.set(key, 'read', `err:${m.err}`);
    return { key, err: m.err };
  }
  w.set(key, 'read', 'ok');
  w.set(key, 'pres', textDigest(m.pres));
  w.set(key, 'slides', m.slides.length);
  const h = createHash('sha256').update(m.pres);
  m.slides.forEach((s, i) => {
    w.set(key, `s${i}`, typeof s === 'string' ? textDigest(s) : `err:${s.err}`);
    h.update('\n').update(typeof s === 'string' ? s : `err:${s.err}`);
  });
  w.set(key, 'model', h.digest('hex').slice(0, 32));
  w.set(key, 'dropped', [...m.dropped].sort().map(([k, n]) => `${k}:${n}`).join(',') || 'none');
  // slides without `timing` (read before parseSlideTiming is ported)
  const hn = createHash('sha256').update(m.pres);
  m.nts.forEach((t, i) => {
    if (t === null) { hn.update('\n').update(`err:${m.slides[i].err}`); return; }
    w.set(key, `n${i}`, textDigest(t));
    hn.update('\n').update(t);
  });
  w.set(key, 'nmodel', hn.digest('hex').slice(0, 32));
  return { key, m };
}

const detail = {};
const writer = new CaptureWriter([
  'Slides read path (B1): canonical JSON of the TS Presentation / Slide models per deck; see js/capture-model.mjs and core/docs/modules/uo-pptx-captures.md',
]);
writer.meta('ts-root-commit', tsCommit(root));
writer.meta('canon', `canon-json.mjs: sorted keys (UTF-16), shortest round-trip numbers, -0 kept, non-finite as {"$num"}, bytes as {"$bytes","sha256"}, strings over ${LONG_TEXT} units as {"$text","sha256"}, undefined/function keys omitted`);

const results = [];
let mode;
if (a.flag('--samples')) {
  mode = 'samples';
  const all = trackedSamples();
  writer.meta('corpus', `slides/samples tracked files: ${all.length}`);
  for (const rel of all) {
    const key = `samples/${rel}`;
    if (only && !key.includes(only)) continue;
    results.push(record(writer, key, readSample(rel)));
  }
} else if (a.opt('--gen')) {
  const names = a.opt('--gen').split(',');
  mode = names.join('+');
  const decksDir = a.opt('--decks');
  const cover = new Map();
  for (const name of names) {
    const g = GROUPS[name];
    if (!g) throw new Error(`unknown group ${name}`);
    writer.meta(`group-${name}`, `seeds ${g.seedBase}..${g.seedBase + g.count - 1} decks ${g.count}`);
    for (let i = 0; i < g.count; i++) {
      const key = groupKey(name, i);
      if (only && !key.includes(only)) continue;
      const d = buildGroupDeck(name, i);
      for (const f of d.features) cover.set(f, (cover.get(f) ?? 0) + 1);
      const bytes = decksDir ? new Uint8Array(readFileSync(join(decksDir, name, deckFile(name, i)))) : new Uint8Array(d.bytes);
      results.push(record(writer, key, bytes, { seed: d.seed }));
    }
  }
  writer.meta('coverage', [...cover].sort().map(([f, n]) => `${f}=${n}`).join(';'));
} else {
  console.error('usage: capture-model.mjs (--samples | --gen <group[,group]>) [--out file] [--detail file.json.gz] [--only s] [--decks dir]');
  process.exit(2);
}

if (detailPath) {
  const parts = results.filter((r) => r.m).sort((x, y) => (x.key < y.key ? -1 : 1)).map((r) => `${JSON.stringify(r.key)}:{"pres":${r.m.pres},"slides":[${r.m.slides.map((s) => (typeof s === 'string' ? s : JSON.stringify({ err: s.err }))).join(',')}]}`);
  mkdirSync(dirname(detailPath), { recursive: true });
  // gzip header carries no mtime (Node writes 0): same input -> same bytes for one zlib build
  writeFileSync(detailPath, gzipSync(Buffer.from(`{${parts.join(',')}}\n`, 'utf8'), { level: 9 }));
}

const out = a.opt('--out', join(capturesDir, `model-${mode}.capture`));
mkdirSync(dirname(out), { recursive: true });
writer.write(out);
const bad = results.filter((r) => r.err !== undefined || r.m?.slides.some((s) => typeof s !== 'string'));
console.log(`${mode}: ${results.length} decks -> ${out}; ${bad.length} with read errors${bad.length ? ': ' + bad.slice(0, 5).map((r) => r.key).join(', ') : ''}`);
