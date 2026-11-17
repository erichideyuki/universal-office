// B5b capture of the picture rasterizers of the frozen TS (`raster.ts` `rasterizeSpecialImage`: metafiles, PICT, TIFF) over the recording
// context: every case of the synthetic `b5b` input set (lib/gen/meta-cases.mjs) is rasterized on a recorder canvas and the canvas's
// call stream (the metafile layer's own lines and its offscreen canvases', `@cvK`) is stored; TIFF cases store the RGBA digest.
//
//   node core/crates/uo-pptx/js/capture-meta.mjs [--out <stem>] [--only <substring>] [--max <n>] [--ts-root <dir>]
//   writes <stem>.inputs (the files: `u32 keyLen key u32 n bytes` repeated), <stem>.ops.gz, <stem>.capture
//
// Stream file: per case and display `#meta <key> display=<w>x<h>|- result=<canvas WxH|none|tiff WxH sha> lines=<n> sha=<h>` then the lines.
// The metafile module is imported FRESH per case (`?run=N`): its font-availability memo (`fontAvail`) is module state, and a shared
// instance would make a case's stream depend on the cases before it (the same reason lib/ts-paint.mjs imports render.js fresh).
import { mkdirSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { gzipSync } from 'node:zlib';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { args, capturesDir, oneLine, tsCommit, tsRoot } from './lib/common.mjs';
import { createRecorder } from './lib/record-ctx.mjs';
import { metaCases } from './lib/gen/meta-cases.mjs';
import { packInputs } from './lib/meta-io.mjs';

const a = args();
const root = tsRoot(a);
const only = a.opt('--only');
const max = Number(a.opt('--max', '1e9'));
const stem = a.opt('--out', join(capturesDir, 'meta-b5b'));
const dist = (n, q = '') => import(pathToFileURL(join(root, 'slides', 'packages', 'slides-core', 'dist', n)).href + q);
const pict = await dist('pict.js');
const tiff = await dist('tiff.js');
const eot = await dist('eot.js');

let runs = 0;
const writer = new CaptureWriter(['B5b: TS picture rasterizers over the recording context (raster.ts rasterizeSpecialImage); see js/capture-meta.mjs']);
writer.meta('ts-root-commit', tsCommit(root));
writer.meta('stream', 'record-ctx.mjs full text of the layer canvas and its offscreen canvases, measureText answered by the stub; EMF+ compressed bitmaps are not decoded (Node has no createImageBitmap)');
const streams = [];
const files = [];
let nCases = 0, nLines = 0, nNone = 0;
const t0 = Date.now();

/** `rasterizeSpecialImage(bytes, makeCanvas, display)` of raster.ts over a recorder, with a fresh metafile module. */
export async function rasterizeCase(bytes, display, extra = {}, key = '') {
  const meta = await dist('metafile.js', `?run=${++runs}`);
  const rec = createRecorder({ width: 8, height: 8, ...extra });
  rec.install();
  let result = 'none';
  let tiffSha = '';
  try {
    if (key.startsWith('eot')) {
      const sf = eot.eotToSfnt(bytes);
      if (sf) { result = `eot ${sf.length} ${createHash('sha1').update(sf).digest('hex').slice(0, 16)}`; }
      return { result, tiffSha, text: '', lines: 0 };
    }
    const mk = (w, h) => rec.createCanvas(w, h);
    let cv = meta.rasterizeMetafile(bytes, mk, display ? { display } : {});
    if (!cv && pict.isPict(bytes)) cv = pict.rasterizePict(bytes, mk);
    if (cv) result = `canvas ${cv.width}x${cv.height}`;
    else if (tiff.isTiff(bytes)) {
      let img;
      try { img = tiff.decodeTiff(bytes); } catch { img = undefined; }
      if (img) {
        // `rasterizeSpecialImage`: the decoded pixels on a canvas of their own
        const c = mk(img.width, img.height);
        const cx = c?.getContext('2d');
        if (c && cx) {
          const id = cx.createImageData(img.width, img.height);
          id.data.set(img.data);
          cx.putImageData(id, 0, 0);
          result = `canvas ${c.width}x${c.height}`;
          tiffSha = createHash('sha1').update(img.data).digest('hex').slice(0, 16);
        }
      }
    }
  } finally {
    rec.uninstall();
  }
  return { result, tiffSha, text: rec.text(), lines: rec.lines.length };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  for (const c of metaCases()) {
    if (only && !c.key.includes(only)) continue;
    if (nCases >= max) break;
    files.push([c.key, c.bytes]);
    for (const d of c.displays) {
      nCases++;
      const r = await rasterizeCase(c.bytes, d, {}, c.key);
      if (r.result === 'none') nNone++;
      const sha = createHash('sha1').update(r.text).digest('hex').slice(0, 16);
      nLines += r.lines;
      const disp = d ? `${d.w}x${d.h}` : '-';
      streams.push(`#meta ${c.key} display=${disp} result=${r.result}${r.tiffSha ? ` ${r.tiffSha}` : ''} lines=${r.lines} sha=${sha}\n`, r.text);
      writer.set(c.key, `d.${disp}`, `${oneLine(r.result)}:${r.lines}:${sha}${r.tiffSha ? `:${r.tiffSha}` : ''}`);
    }
  }
  mkdirSync(dirname(stem), { recursive: true });
  writer.write(`${stem}.capture`);
  const gz = gzipSync(Buffer.from(streams.join(''), 'utf8'), { level: 9 });
  writeFileSync(`${stem}.ops.gz`, gz);
  const inputs = packInputs(files);
  writeFileSync(`${stem}.inputs`, inputs);
  console.log(`${files.length} files, ${nCases} rasterizations (${nNone} not special), ${nLines} lines, ops.gz ${gz.length} B, inputs ${inputs.length} B, ${((Date.now() - t0) / 1000).toFixed(1)} s -> ${stem}.*`);
}
