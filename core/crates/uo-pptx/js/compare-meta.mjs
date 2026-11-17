// B5b parity of the picture rasterizers: the Rust recipes (`uo-dml-render/examples/meta_recipe.rs`) replayed by `meta-player.ts` over the
// recording context against the TS streams of `capture-meta.mjs`, line by line (the whole text: state sets, measure probes, offscreen
// canvases). TIFF cases compare the RGBA digest.
//
//   node core/crates/uo-pptx/js/compare-meta.mjs [--stem <capture stem>] [--exe <meta_recipe>] [--only <s>] [--show 5] [--keep <dir>]
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { args, capturesDir, crateDir } from './lib/common.mjs';
import { createRecorder } from './lib/record-ctx.mjs';
import { unpackOutputs } from './lib/meta-io.mjs';
import { replayRecipe } from '../../uo-wasm/js/meta-player.ts';

const a = args();
const stem = a.opt('--stem', join(capturesDir, 'meta-b5b'));
const exe = a.opt('--exe', join(crateDir, '..', '..', 'target', 'b5b', 'release', 'examples', process.platform === 'win32' ? 'meta_recipe.exe' : 'meta_recipe'));
const only = a.opt('--only');
const show = Number(a.opt('--show', '5'));
const keep = a.opt('--keep', join(crateDir, '..', '..', 'target', 'b5b', 'scratch'));
mkdirSync(keep, { recursive: true });

// the TS streams
const text = gunzipSync(readFileSync(`${stem}.ops.gz`)).toString('utf8');
const entries = [];
let cur = null;
for (const line of text.split('\n')) {
  if (line.startsWith('#meta ')) {
    const m = /^#meta (\S+) display=(\S+) result=(.*?) lines=(\d+) sha=(\w+)$/.exec(line);
    cur = { key: m[1], display: m[2], result: m[3], lines: [], n: Number(m[4]), sha: m[5] };
    entries.push(cur);
  } else if (cur) cur.lines.push(line);
}
for (const e of entries) if (e.lines.length && e.lines[e.lines.length - 1] === '') e.lines.pop();
// the `measure` lines are the probes of `measureText`: the Rust side asks them of the host (the table), the replay has no such call
const noMeasure = (lines) => lines.filter((l) => !/^(@cv\d+ )?measure /.test(l));
for (const e of entries) e.lines = noMeasure(e.lines);
const sel = entries.filter((e) => !only || e.key.includes(only));
const manifest = sel.map((e) => `${e.key} ${e.display === '-' ? '- -' : e.display.replace('x', ' ')}`).join('\n') + '\n';
writeFileSync(join(keep, 'meta.manifest'), manifest);
const t0 = Date.now();
execFileSync(exe, [`${stem}.inputs`, join(keep, 'meta.manifest'), join(keep, 'meta.out')], { stdio: ['ignore', 'inherit', 'inherit'], windowsHide: true });
const tRust = Date.now() - t0;
const outs = unpackOutputs(readFileSync(join(keep, 'meta.out')));

const stats = new Map();
let shown = 0;
const bump = (kind, field) => { const s = stats.get(kind) ?? { total: 0, equal: 0, different: 0 }; s[field]++; stats.set(kind, s); };
sel.forEach((e, i) => {
  const o = outs[i];
  const kind = e.key.split('/')[0];
  bump(kind, 'total');
  let ok = false, why = '';
  if (!o || o.key !== e.key) why = 'missing output';
  else if (e.result === 'none') { ok = o.status === 0; why = `TS none, Rust status ${o.status}`; }
  else if (e.result.startsWith('eot')) {
    if (o.status !== 4) why = `TS ${e.result}, Rust status ${o.status}`;
    else {
      const sha = createHash('sha1').update(o.bytes).digest('hex').slice(0, 16);
      ok = e.result === `eot ${o.bytes.length} ${sha}`;
      if (!ok) why = `TS ${e.result} vs Rust eot ${o.bytes.length} ${sha}`;
    }
  } else if (o.status !== 1) why = `TS ${e.result}, Rust status ${o.status}`;
  else {
    const rec = createRecorder({ width: 8, height: 8 });
    rec.install();
    let canvas;
    try { canvas = replayRecipe(o.recipe, { createCanvas: (w, h) => rec.createCanvas(w, h) }); } finally { rec.uninstall(); }
    const got = rec.lines.slice();
    const size = canvas ? `canvas ${canvas.width}x${canvas.height}` : 'null';
    if (size !== e.result.split(' ').slice(0, 2).join(' ')) why = `size ${e.result} (TS) vs ${size} (Rust)`;
    else {
      let k = 0;
      while (k < got.length && k < e.lines.length && got[k] === e.lines[k]) k++;
      if (k === got.length && k === e.lines.length) ok = true;
      else why = `line ${k + 1} of ${e.lines.length} (TS) / ${got.length} (Rust)\n  TS:   ${e.lines[k]}\n  Rust: ${got[k]}`;
    }
  }
  if (ok) bump(kind, 'equal');
  else {
    bump(kind, 'different');
    if (shown++ < show) console.log(`DIFFERS ${e.key} display=${e.display}: ${why}`);
  }
});
let eq = 0, tot = 0;
for (const [k, s] of [...stats].sort()) { console.log(`${k.padEnd(14)} ${String(s.equal).padStart(5)} / ${String(s.total).padEnd(5)} equal${s.different ? `, ${s.different} different` : ''}`); eq += s.equal; tot += s.total; }
console.log(`${eq} / ${tot} equal (Rust ${(tRust / 1000).toFixed(1)} s incl. process start; replay + compare ${(((Date.now() - t0) - tRust) / 1000).toFixed(1)} s)`);
process.exit(eq === tot ? 0 : 1);
