// Parity capture of the Slides link boxes (wave 4, record uo-dml-render.md "Links (wave 4)"): the boxes the frozen TS viewer
// hit-tests (`hitTestHyperlink` of slides/packages/viewer/src/universal-slides-viewer.ts: every shape with `xfrm` and
// `txBody` whose runs hold a `hyperlink`, its unrotated box at the fit, the address of the first such run), over the tracked
// .pptx samples and the committed generated decks, at one fixed view. The Rust side is `uo_pptx::links::slide_links`
// (`LinkSource::RunUrl`), compared by `tests/links.rs`.
//
//   node core/crates/uo-pptx/js/capture-links.mjs            # writes tests/captures/links.tsv
//   options: --out <file>  --ts-root <dir> (default UO_TS_ROOT, else the 0.9.2 reference)
//
// Lines: `key<TAB>slide<TAB>shape<TAB>x<TAB>y<TAB>w<TAB>h<TAB>url` (numbers as JS `String(n)`, which round-trips the double;
// `slide` is the presentation index), or `key<TAB>slide<TAB>err` when readSlide throws; a header line starts with `#`.
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { args, capturesDir, loadSlidesCore, oneLine, readSample, trackedSamples, tsCommit, tsRoot } from './lib/common.mjs';

const a = args();
const root = tsRoot(a);
const core = await loadSlidesCore(root);
const out = a.opt('--out', join(capturesDir, 'links.tsv'));

/** The fixed view of the capture (px per EMU and origin), the same in tests/links.rs. */
const VIEW = { scale: 96 / 914400, ox: 3, oy: 5 };
const GROUPS = ['b1a', 'b3a', 'b5b', 'b6a', 'c1d'];

const lines = [`# capture-links ts=${tsCommit(root)} scale=96/914400 ox=3 oy=5`];
let decks = 0, boxes = 0;

function deck(key, bytes) {
  let pkg, pres;
  try {
    pkg = core.PptxPackage.load(bytes);
    pres = core.readPresentation(pkg);
  } catch {
    return;
  }
  decks++;
  pres.slides.forEach((ref, si) => {
    if (!ref.target) return;
    let slide;
    try { slide = core.readSlide(pkg, ref.target); } catch (e) { lines.push(`${key}\t${si}\terr:${oneLine(e?.message ?? e)}`); return; }
    slide.shapes.forEach((sh, i) => {
      if (!sh.xfrm || !sh.txBody) return;
      let url = null;
      for (const p of sh.txBody.paragraphs) {
        for (const r of p.runs) if (url === null && r.hyperlink) url = r.hyperlink;
      }
      if (url === null) return;
      const x = VIEW.ox + sh.xfrm.x * VIEW.scale, y = VIEW.oy + sh.xfrm.y * VIEW.scale;
      const w = sh.xfrm.cx * VIEW.scale, h = sh.xfrm.cy * VIEW.scale;
      lines.push([key, si, i, String(x), String(y), String(w), String(h), url.replace(/[\t\r\n]/g, ' ')].join('\t'));
      boxes++;
    });
  });
}

for (const rel of trackedSamples()) if (/\.pptx$/i.test(rel)) deck(`samples/${rel}`, readSample(rel));
for (const g of GROUPS) {
  const dir = join(capturesDir, 'decks', g);
  for (const f of readdirSync(dir).filter((n) => n.endsWith('.pptx')).sort()) deck(`${g}/${f}`, new Uint8Array(readFileSync(join(dir, f))));
}
writeFileSync(out, lines.join('\n') + '\n');
console.log(`capture-links: ${decks} decks, ${boxes} link boxes -> ${out}`);
