// Draw-op stream capture of the frozen TS painter (plan section 4.3, B4's primary gate): every slide of the committed
// samples and of the generated decks is painted by `paintSlideTo` (and, for the first view only, `paintShapesTo`) of the
// frozen slides-core over the recording context (lib/record-ctx.mjs). Output: the canonical op streams (gzip text) and a
// uo-testkit capture with one digest facet per slide and view.
//
//   node core/crates/uo-pptx/js/capture-drawops.mjs --samples                       # the tracked samples
//   node core/crates/uo-pptx/js/capture-drawops.mjs --gen b1a --decks core/crates/uo-pptx/tests/captures/decks
//   options: --out <stem>     writes <stem>.capture, <stem>.ops.gz, <stem>.measures.tsv.gz
//            --views a,b      views to record (default: a,b; `a` = 1280 px wide at the origin, `b` = 640 px wide at (17.5, 9.25))
//            --caps nolayers  a capability set (`nolayers`, `nopattern`, `nospacing`, `nokerning`, comma separated)
//            --only <s>       substring of the key        --max <n>   at most n decks
//            --ts-root <dir>  the frozen checkout (default UO_TS_ROOT, else the 0.9.2 reference)
//
// Stream file: `#deck <key>` then per slide `#slide <N> view=<v> scale=<s> ox=<ox> oy=<oy> canvas=<w>x<h> lines=<n> sha=<h>`
// followed by the stream lines; a slide that throws has `#error <message>` after its header. The capture records per
// deck `read`, `slides`, then `s<N>.<view>=<lines>:<sha>` (and `s<N>.shapes=<lines>:<sha>` for view a, paintShapesTo).
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { gzipSync } from 'node:zlib';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { args, capturesDir, loadSlidesCore, oneLine, readSample, trackedSamples, tsCommit, tsRoot } from './lib/common.mjs';
import { GROUPS, buildGroupDeck, deckFile, groupKey } from './lib/gen/groups.mjs';
import { recordTsSlide } from './lib/ts-paint.mjs';
import { digest } from './lib/opstream.mjs';
import { VIEWS, parseCaps } from './lib/views.mjs';

const a = args();
const root = tsRoot(a);
const core = await loadSlidesCore(root);
const only = a.opt('--only');
const max = Number(a.opt('--max', '1e9'));
const views = a.opt('--views', 'a,b').split(',');
const caps = parseCaps(a.opt('--caps'));
const writer = new CaptureWriter(['Slides draw-op streams of the frozen TS painter (R-S4); see js/capture-drawops.mjs and core/docs/modules/uo-render.md "R-S3/R-S4"']);
writer.meta('ts-root-commit', tsCommit(root));
writer.meta('stream', 'record-ctx.mjs canonical op stream in the DRAW form (lib/opstream.mjs); per slide `lines:sha1[0..16]` of the draw-form text; views a/b per capture-drawops.mjs');
writer.meta('caps', Object.entries(caps).map(([k, v]) => `${k}=${v}`).join(','));
writer.meta('measure', 'record-ctx.mjs stubMeasure (deterministic advance table); measureText answers are in the .measures.tsv.gz file');

const streams = [];
const measures = new Map();
let nDecks = 0, nSlides = 0, nLines = 0, nFull = 0, nErr = 0;
const t0 = Date.now();

async function record(key, bytes, extra = {}) {
  if (only && !key.includes(only)) return;
  if (nDecks >= max) return;
  nDecks++;
  for (const [k, v] of Object.entries(extra)) writer.set(key, k, v);
  let pkg, pres;
  try { pkg = core.PptxPackage.load(bytes); pres = core.readPresentation(pkg); } catch (e) { writer.set(key, 'read', `err:${oneLine(e?.message ?? e)}`); return; }
  writer.set(key, 'read', 'ok');
  writer.set(key, 'slides', pres.slides.length);
  streams.push(`#deck ${key}\n`);
  for (let i = 0; i < pres.slides.length; i++) {
    const ref = pres.slides[i];
    let slide;
    try { slide = core.readSlide(pkg, ref.target); } catch (e) { writer.set(key, `s${i}`, `err:${oneLine(e?.message ?? e)}`); continue; }
    for (const v of views) {
      const view = VIEWS[v](pres);
      const r = await recordTsSlide({ tsRoot: root, slide, pres, caps, ...view });
      const d = await digest(r.text);
      nSlides++; nLines += d.lines; nFull += r.lines;
      if (r.error) nErr++;
      const size = `${Math.ceil(pres.slideW * view.scale + 2 * view.ox)}x${Math.ceil(pres.slideH * view.scale + 2 * view.oy)}`;
      streams.push(`#slide ${i} view=${v} scale=${view.scale} ox=${view.ox} oy=${view.oy} canvas=${size} lines=${d.lines} sha=${d.sha}\n`);
      if (r.error) streams.push(`#error ${oneLine(r.error)}\n`);
      streams.push(d.text);
      writer.set(key, `s${i}.${v}`, r.error ? `err:${oneLine(r.error)}` : `${d.lines}:${d.sha}`);
      for (const m of r.measures) {
        const f = m.split('\t');
        const k = `${f[0]}\t${f[1]}`;
        const e = measures.get(k);
        if (e) e.n += +f[3]; else measures.set(k, { w: f[2], n: +f[3] });
      }
      if (v === views[0]) {
        const s = await recordTsSlide({ tsRoot: root, slide, pres, caps, mode: 'shapes', ...view });
        writer.set(key, `s${i}.shapes`, s.error ? `err:${oneLine(s.error)}` : `${(await digest(s.text)).lines}:${(await digest(s.text)).sha}`);
      }
    }
  }
}

if (a.flag('--samples')) {
  for (const rel of trackedSamples()) await record(`samples/${rel}`, readSample(rel));
} else if (a.opt('--gen')) {
  const decksDir = a.opt('--decks');
  for (const name of a.opt('--gen').split(',')) {
    const g = GROUPS[name];
    if (!g) throw new Error(`unknown group ${name}`);
    for (let i = 0; i < g.count; i++) {
      const d = decksDir ? null : buildGroupDeck(name, i);
      const bytes = decksDir ? new Uint8Array(readFileSync(join(decksDir, name, deckFile(name, i)))) : new Uint8Array(d.bytes);
      await record(groupKey(name, i), bytes);
    }
  }
} else {
  console.error('usage: capture-drawops.mjs (--samples | --gen <group>) [--out stem] [--views a,b] [--caps list] [--only s] [--max n] [--decks dir]');
  process.exit(2);
}

const stem = a.opt('--out', join(capturesDir, `drawops-${a.flag('--samples') ? 'samples' : a.opt('--gen')}`));
mkdirSync(dirname(stem), { recursive: true });
writer.write(`${stem}.capture`);
const gz = gzipSync(Buffer.from(streams.join(''), 'utf8'), { level: 9 });
writeFileSync(`${stem}.ops.gz`, gz);
const mt = [...measures].sort((x, y) => (x[0] < y[0] ? -1 : 1)).map(([k, v]) => `${k}\t${v.w}\t${v.n}`).join('\n');
const mgz = gzipSync(Buffer.from(mt + '\n', 'utf8'), { level: 9 });
writeFileSync(`${stem}.measures.tsv.gz`, mgz);
console.log(`${nDecks} decks, ${nSlides} slide views, ${nLines} draw-form lines (${nFull} full), ${nErr} errors, ${measures.size} distinct measureText keys; ` +
  `ops.gz ${gz.length} B, measures.gz ${mgz.length} B, ${((Date.now() - t0) / 1000).toFixed(1)} s -> ${stem}.*`);
