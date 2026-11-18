// M3 reference of the retained slide model: the heap the frozen TS `readSlide` keeps alive, measured the
// way `tests/memory.rs` measures the Rust `SlideModel` (`m3_retained_slide_model_*`):
//   - `0008/0`: slide 0 of the generated deck b1a/0008 (9 shapes), 20 copies alive at once, heap delta / 20,
//     median of 5 runs;
//   - per corpus group (the generated groups under tests/captures/decks/<group> and the tracked .pptx
//     samples): every slide of every deck read once and kept alive, the packages loaded and each slide read
//     once before (warm caches), heap delta, median of 3 runs.
// The heap is `heapUsed` after two full GCs (`node --expose-gc`). `ImageRef.bytes` points at the package's
// bytes in the TS (not counted, as the Rust media table entries are not counted).
//
//   node --expose-gc core/crates/uo-pptx/js/measure-memory.mjs [--ts-root <dir>] [--groups b1a,samples,...]
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { args, capturesDir, loadSlidesCore, readSample, trackedSamples, tsRoot } from './lib/common.mjs';

if (typeof globalThis.gc !== 'function') throw new Error('run with node --expose-gc');
const a = args();
const core = await loadSlidesCore(tsRoot(a));
const heap = () => { globalThis.gc(); globalThis.gc(); return process.memoryUsage().heapUsed; };
const median = (v) => v.slice().sort((x, y) => x - y)[v.length >> 1];

function groupDecks(group) {
  if (group === 'samples') {
    return trackedSamples().filter((s) => s.toLowerCase().endsWith('.pptx')).map((s) => readSample(s));
  }
  const dir = join(capturesDir, 'decks', group);
  return readdirSync(dir).filter((f) => f.endsWith('.pptx')).sort().map((f) => new Uint8Array(readFileSync(join(dir, f))));
}

/** Loaded packages with their readable slide targets (decks or slides the TS cannot read are skipped). */
function openAll(decks) {
  const out = [];
  for (const bytes of decks) {
    let pkg, pres;
    try { pkg = core.PptxPackage.load(bytes); pres = core.readPresentation(pkg); } catch { continue; }
    const targets = [];
    for (const ref of pres.slides) {
      if (ref.target === undefined) continue;
      try { core.readSlide(pkg, ref.target); targets.push(ref.target); } catch { /* unreadable: skipped on both sides */ }
    }
    out.push({ pkg, targets });
  }
  return out;
}

// 0008 slide 0, 20 copies
{
  const [{ pkg, targets }] = openAll(groupDecks('b1a').slice(8, 9));
  const runs = [];
  for (let r = 0; r < 5; r++) {
    const keep = [];
    const before = heap();
    for (let i = 0; i < 20; i++) keep.push(core.readSlide(pkg, targets[0]));
    const after = heap();
    runs.push((after - before) / 20);
    if (keep.length !== 20) throw new Error('unreachable');
  }
  console.log(`0008/0 | shapes ${core.readSlide(pkg, targets[0]).shapes.length} | TS heap per slide ${Math.round(median(runs))}`);
}

const groups = (a.opt('--groups') ?? 'b1a,b3a,b5a,b5b,b6a,c1d,samples').split(',');
for (const g of groups) {
  const opened = openAll(groupDecks(g));
  let slides = 0, shapes = 0;
  for (const { pkg, targets } of opened) for (const t of targets) { slides++; shapes += core.readSlide(pkg, t).shapes.length; }
  const runs = [];
  for (let r = 0; r < 3; r++) {
    const keep = [];
    const before = heap();
    for (const { pkg, targets } of opened) for (const t of targets) keep.push(core.readSlide(pkg, t));
    const after = heap();
    runs.push(after - before);
    if (keep.length !== slides) throw new Error('unreachable');
  }
  console.log(`${g} | decks ${opened.length} | slides ${slides} | shapes ${shapes} | TS heap ${median(runs)}`);
}
