// A/B of viewer modules in ONE Node process (core/docs/modules/size-optimization.md): the runs of every module are
// interleaved, so machine load hits all of them alike (separate processes on this machine varied by up to 1.8x).
// Per module: the sum over 4 fixtures of the minimum and the median of `open`, layout, pages, PDF and total (ms), and
// the digest of everything a viewer reads from the 12 committed fixtures (tests/wasm2js_common.mjs `collect`): equal
// digests = byte-identical page messages, Write sections, annotations, navigation, link boxes, page info, face, PDFs.
//   node core/tools/size/wasm-ab.mjs [--runs 21] <base.wasm> <new.wasm> [...]
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadWrite } from '../../crates/uo-wasm/js/write.mjs';
import * as c from '../../crates/uo-wasm/tests/wasm2js_common.mjs';

const repo = join(dirname(fileURLToPath(import.meta.url)), '../../..');
const args = process.argv.slice(2);
const ri = args.indexOf('--runs');
const runs = ri < 0 ? 21 : Number(args[ri + 1]);
const files = args.filter((_, i) => i !== ri && i !== ri + 1);
const keys = ['f8__7__0', 'f8__7__10', 'f8__7__9', 'f8__7__7'];
const assets = await c.loadAssets(async (p) => new Uint8Array(readFileSync(join(repo, p))));
const libs = [];
for (const f of files) libs.push(await loadWrite({ engine: 'wasm', wasm: new Uint8Array(readFileSync(f)) }));
const digests = libs.map((l) => createHash('sha256').update(c.FIXTURES.map((k) => `${k}: ${c.digest(c.collect(l.uo, assets, k))}`).join('\n')).digest('hex').slice(0, 16));
const res = files.map(() => ({}));
for (const k of keys) {
  const st = libs.map((l) => { const env = c.newEnv(l.uo, assets); const doc = l.uo.open(assets.docs[k]); c.timeOnce(env, doc); return { env, doc }; });
  const rows = files.map(() => []);
  for (let r = 0; r < runs; r++) {
    for (let i = 0; i < libs.length; i++) {
      const row = c.timeOnce(st[i].env, st[i].doc);
      const t = performance.now(); const d = libs[i].uo.open(assets.docs[k]); row.open = performance.now() - t; d.free();
      rows[i].push(row);
    }
  }
  st.forEach((s) => { s.doc.free(); s.env.free(); });
  for (let i = 0; i < libs.length; i++) {
    for (const f of ['open', 'layout', 'pages', 'pdf', 'total']) {
      const xs = rows[i].map((x) => x[f]).sort((a, b) => a - b);
      res[i][f] = res[i][f] ?? [0, 0];
      res[i][f][0] += xs[0]; res[i][f][1] += c.median(xs);
    }
  }
}
const base = res[0];
for (let i = 0; i < files.length; i++) {
  const cell = (f) => { const [mn, md] = res[i][f]; const d = i ? ` (${((100 * (mn - base[f][0])) / base[f][0]).toFixed(1)}%)` : ''; return `${f} ${mn.toFixed(1)}${d} / ${md.toFixed(1)}`; };
  console.log(`${basename(files[i]).padEnd(18)} digest ${digests[i]}${i && digests[i] !== digests[0] ? ' DIFFERS' : ''} | min (vs first) / median, ms: ${['open', 'layout', 'pages', 'pdf', 'total'].map(cell).join(' | ')}`);
}
