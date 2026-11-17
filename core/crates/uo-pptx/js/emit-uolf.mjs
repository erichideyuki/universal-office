// B4 parity driver: for every deck of a TS draw-op capture (`capture-drawops.mjs`, `.ops.gz`) run the Rust emitter
// (`cargo run -p uo-pptx --example emit_uolf`) and write `<out>/<key with / as __>.uolf`, the frame files
// `compare-drawops.mjs` reads. The frames follow the capture entry by entry (slide, view, scale, origin as the capture
// recorded them), so the comparison needs no other input.
//
//   node core/crates/uo-pptx/js/emit-uolf.mjs --ts core/crates/uo-pptx/tests/captures/drawops-samples.ops.gz --out <dir>
//        [--exe <path to emit_uolf.exe>] [--decks <dir>] [--only <key substring>] [--max <n>]
//   then: node core/crates/uo-pptx/js/compare-drawops.mjs --ts <the same capture> --rust <dir>
//
// `--exe` defaults to `core/target/b4/release/examples/emit_uolf(.exe)` (build it with the B4 CARGO_TARGET_DIR).
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { args, capturesDir, crateDir, readSample } from './lib/common.mjs';
import { GROUPS, buildGroupDeck, deckFile } from './lib/gen/groups.mjs';
import { parseOps } from './lib/opstream.mjs';

const a = args();
const tsPath = a.opt('--ts');
const outDir = a.opt('--out');
if (!tsPath || !outDir) {
  console.error('usage: emit-uolf.mjs --ts <capture.ops.gz> --out <dir> [--exe path] [--decks dir] [--only s] [--max n]');
  process.exit(2);
}
const exe = a.opt('--exe', join(crateDir, '..', '..', 'target', 'b4', 'release', 'examples', process.platform === 'win32' ? 'emit_uolf.exe' : 'emit_uolf'));
const decksDir = a.opt('--decks', join(capturesDir, 'decks'));
const only = a.opt('--only');
const max = Number(a.opt('--max', '1e9'));
mkdirSync(outDir, { recursive: true });
const tmp = join(outDir, '_decks');
mkdirSync(tmp, { recursive: true });

const entries = parseOps(gunzipSync(readFileSync(tsPath)).toString('utf8'));
const byDeck = new Map();
for (const e of entries) { if (!byDeck.has(e.deck)) byDeck.set(e.deck, []); byDeck.get(e.deck).push(e); }

function deckBytes(key) {
  if (key.startsWith('samples/')) return readSample(key.slice('samples/'.length));
  const m = /^gen\/([^/]+)\/(\d+)$/.exec(key);
  if (!m) throw new Error(`unknown deck key ${key}`);
  const file = join(decksDir, m[1], deckFile(m[1], Number(m[2])));
  if (existsSync(file)) return new Uint8Array(readFileSync(file));
  if (!GROUPS[m[1]]) throw new Error(`unknown group ${m[1]}`);
  return new Uint8Array(buildGroupDeck(m[1], Number(m[2])).bytes);
}

let n = 0, failed = 0;
const t0 = Date.now();
for (const [key, slides] of byDeck) {
  if (only && !key.includes(only)) continue;
  if (n >= max) break;
  n++;
  const base = key.replace(/\//g, '__');
  const deckFileName = join(tmp, `${base}.pptx`);
  const manifest = join(tmp, `${base}.txt`);
  writeFileSync(deckFileName, deckBytes(key));
  writeFileSync(manifest, slides.map((s) => `${s.slide} ${s.view} ${s.header.scale} ${s.header.ox} ${s.header.oy}`).join('\n') + '\n');
  try {
    execFileSync(exe, [deckFileName, manifest, join(outDir, `${base}.uolf`)], { stdio: ['ignore', 'ignore', 'pipe'], windowsHide: true });
  } catch (e) {
    failed++;
    console.log(`emit failed ${key}: ${String(e.stderr ?? e.message).split('\n').slice(0, 2).join(' | ')}`);
  }
}
console.log(`${n} decks emitted (${failed} with failures) in ${((Date.now() - t0) / 1000).toFixed(1)} s -> ${outDir}`);
