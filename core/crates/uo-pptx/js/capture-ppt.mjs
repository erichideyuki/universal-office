// Parity capture of the TS .ppt -> .pptx converter (plan section 4.3 and B2): for every tracked
// binary .ppt sample, the parts of the in-memory .pptx `convertPptToPptx` builds, as
// `slides/tools/ppt-lab/model-regress.mjs` hashes them, but strict: the part names in archive
// order, SHA-256 of every part, and the bytes of the small parts (so a mismatch can be diffed
// without the TS at hand). Only git-tracked samples are used (no Office here; the .ppt files
// that PowerPoint COM would save from generated decks are a later, Windows-only step).
//
//   node core/crates/uo-pptx/js/capture-ppt.mjs [--out <file>] [--ts-root <dir>] [--only <substring>]
//
// Writes tests/captures/ppt-samples.capture. Records (key `samples/<path>.ppt`), facets:
//   input     `len:sha256[0..16]` of the .ppt
//   convert   `ok` or `err:<one-line message>` (PptFormatError and any other throw)
//   parts     number of parts, then `|`-joined part names in archive order (`3|a|b|c`)
//   p<N>      part N (archive order): `len:sha256[0..32]`
//   b<N>      (parts of at most SMALL bytes) the part bytes as `z:<base64 of a raw-deflate stream>`;
//             the replay inflates it. Larger parts have only their hash.
//
// FRESH MODULE PER DECK. The TS converter keeps a module-global counter for the GUID of every
// `a:fld` it writes (`fieldSeq` in ppt/textconv.ts, `{B6F15528-21DE-4FAA-801E-634DDDAF<seq:04X>}`,
// never reset), so the bytes of a deck depend on how many fields earlier conversions of the same
// process produced. Each deck here is converted in its own worker thread (a fresh module
// instance: the counter starts at 0, the same as a one-deck process) and twice, to check that the
// parts are then stable. The Rust converter must number fields from 1 per conversion.
// The zip container itself is not stable (fflate stamps the current time in the entry headers),
// so there is no whole-zip hash: the parts are the contract.
import { createHash } from 'node:crypto';
import { mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { Worker, isMainThread, parentPort, workerData } from 'node:worker_threads';
import { fileURLToPath } from 'node:url';
import { deflateRawSync } from 'node:zlib';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { args, capturesDir, loadSlidesCore, oneLine, readSample, trackedSamples, tsCommit, tsRoot } from './lib/common.mjs';
import { bytesDigest } from './lib/canon-json.mjs';

const SMALL = 8192;
// the day the committed capture was taken; override with --today YYYY-MM-DD
const TODAY = '2026-11-17';

function freezeDate(day) {
  const [y, m, d] = day.split('-').map(Number);
  const fixed = new Date(y, m - 1, d, 12, 0, 0).getTime();
  const Real = Date;
  globalThis.Date = class extends Real {
    constructor(...a) { if (a.length) super(...a); else super(fixed); }
    static now() { return fixed; }
  };
}

if (!isMainThread) {
  // worker: convert one deck with a fresh module instance, report the parts in archive order
  // the automatic date field reads `new Date()`: freeze it (local noon of TODAY) so re-runs give the same bytes;
  // the Rust replay pins the same day through ConvertOptions.today
  freezeDate(workerData.today);
  const core = await loadSlidesCore(workerData.root);
  try {
    if (!core.isCfb(workerData.bytes)) throw new Error('not-cfb');
    const pkg = core.PptxPackage.load(core.convertPptToPptx(workerData.bytes));
    parentPort.postMessage({ ok: true, parts: pkg.partNames().map((n) => [n, pkg.getPart(n)]) });
  } catch (e) {
    parentPort.postMessage({ ok: false, err: oneLine(e?.message ?? e) });
  }
} else {
  const a = args();
  const root = tsRoot(a);
  const only = a.opt('--only');
  const today = a.opt('--today', TODAY);
  const convertFresh = (bytes) => new Promise((resolve, reject) => {
    const wk = new Worker(fileURLToPath(import.meta.url), { workerData: { root, bytes, today } });
    wk.once('message', resolve);
    wk.once('error', reject);
  });

  const w = new CaptureWriter(['TS .ppt -> .pptx converter (B2): parts of convertPptToPptx per tracked .ppt sample; see js/capture-ppt.mjs and core/docs/modules/uo-pptx-captures.md']);
  w.meta('ts-root-commit', tsCommit(root));
  w.meta('small-part-bytes', SMALL);
  w.meta('today', today);
  w.meta('counter', 'a:fld GUID counter (ppt/textconv.ts fieldSeq) starts at 0 for every deck (fresh worker per deck)');
  let decks = 0, errors = 0, parts = 0, stored = 0;
  for (const rel of trackedSamples().filter((s) => /\.ppt$/i.test(s))) {
    const key = `samples/${rel}`;
    if (only && !key.includes(only)) continue;
    const bytes = readSample(rel);
    decks++;
    w.set(key, 'input', bytesDigest(bytes));
    const r1 = await convertFresh(bytes);
    if (!r1.ok) { w.set(key, 'convert', `err:${r1.err}`); errors++; continue; }
    const r2 = await convertFresh(bytes);
    const same = r2.ok && r2.parts.length === r1.parts.length && r1.parts.every(([n, d], i) => r2.parts[i][0] === n && Buffer.from(d).equals(Buffer.from(r2.parts[i][1])));
    if (!same) throw new Error(`${key}: convertPptToPptx is not deterministic even with a fresh module`);
    w.set(key, 'convert', 'ok');
    w.set(key, 'parts', `${r1.parts.length}|${r1.parts.map(([n]) => n).join('|')}`);
    r1.parts.forEach(([, d], i) => {
      parts++;
      w.set(key, `p${i}`, `${d.length}:${createHash('sha256').update(d).digest('hex').slice(0, 32)}`);
      if (d.length <= SMALL) { w.set(key, `b${i}`, `z:${deflateRawSync(d, { level: 9 }).toString('base64')}`); stored++; }
    });
  }
  const out = a.opt('--out', join(capturesDir, 'ppt-samples.capture'));
  mkdirSync(dirname(out), { recursive: true });
  w.write(out);
  console.log(`ppt: ${decks} .ppt decks, ${parts} parts (${stored} with bytes), ${errors} errors -> ${out}`);
}
