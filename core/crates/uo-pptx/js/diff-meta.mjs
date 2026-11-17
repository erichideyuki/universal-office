// Debug aid of the B5b harness: rasterize ONE file (a path, or `<stem>.inputs#<key>`) with the frozen TS and the Rust player and print the
// first difference of the two call streams.
//
//   node core/crates/uo-pptx/js/diff-meta.mjs <file | stem.inputs#key> [--display WxH] [--context 6]
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { args, crateDir } from './lib/common.mjs';
import { createRecorder } from './lib/record-ctx.mjs';
import { packInputs, unpackInputs, unpackOutputs } from './lib/meta-io.mjs';
import { replayRecipe } from '../../uo-wasm/js/meta-player.ts';
import { rasterizeCase } from './capture-meta.mjs';

const a = args();
const src = a.argv.find((x) => !x.startsWith('--') && x !== a.opt('--display') && x !== a.opt('--context'));
const ctxN = Number(a.opt('--context', '6'));
const d = a.opt('--display');
const display = d ? { w: Number(d.split('x')[0]), h: Number(d.split('x')[1]) } : null;
let bytes;
if (src.includes('#')) { const [f, k] = src.split('#'); bytes = new Map(unpackInputs(readFileSync(f))).get(k); } else bytes = new Uint8Array(readFileSync(src));
const scratch = join(crateDir, '..', '..', 'target', 'b5b', 'scratch');
const exe = join(crateDir, '..', '..', 'target', 'b5b', 'release', 'examples', 'meta_recipe.exe');
writeFileSync(join(scratch, 'diff.inputs'), packInputs([['x', bytes]]));
writeFileSync(join(scratch, 'diff.manifest'), `x ${display ? `${display.w} ${display.h}` : '- -'}\n`);
execFileSync(exe, [join(scratch, 'diff.inputs'), join(scratch, 'diff.manifest'), join(scratch, 'diff.out')], { stdio: ['ignore', 'ignore', 'inherit'] });
const o = unpackOutputs(readFileSync(join(scratch, 'diff.out')))[0];
const ts = await rasterizeCase(bytes, display);
const noM = (t) => t.split('\n').filter((l) => l && !/^(@cv\d+ )?measure /.test(l));
const tl = noM(ts.text);
let got = [];
let size = `status ${o.status}`;
if (o.status === 1) {
  const rec = createRecorder({ width: 8, height: 8 });
  rec.install();
  const cv = replayRecipe(o.recipe, { createCanvas: (w, h) => rec.createCanvas(w, h) });
  rec.uninstall();
  got = noM(rec.text());
  size = cv ? `canvas ${cv.width}x${cv.height}` : 'null';
}
console.log(`TS: ${ts.result}, ${tl.length} lines; Rust: ${size}, ${got.length} lines`);
let k = 0;
while (k < tl.length && k < got.length && tl[k] === got[k]) k++;
if (k === tl.length && k === got.length) console.log('equal');
else {
  for (let i = Math.max(0, k - ctxN); i < k; i++) console.log(`  ${i + 1} ${tl[i]}`);
  console.log(`line ${k + 1}:\n  TS:   ${tl[k]}\n  Rust: ${got[k]}`);
}
