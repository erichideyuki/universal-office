// Seeded deterministic deck generator (pure Node, no dependencies): writes .pptx decks of the
// generated groups (lib/gen/groups.mjs). Same seed -> same bytes (fixed zip times, zlib deflate
// level 6, no Date/Math.random).
//
//   node core/crates/uo-pptx/js/gen-decks.mjs --list
//   node core/crates/uo-pptx/js/gen-decks.mjs --gen b1a [--out <dir>]      # default: tests/captures/decks/<group>
//   node core/crates/uo-pptx/js/gen-decks.mjs --gen b1-big --out D:/tmp/decks  # local only
//   node core/crates/uo-pptx/js/gen-decks.mjs --gen b1a --check             # committed decks == regenerated
//   options: --only <i[,j..]>  --stats (feature coverage over the group)
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { args, capturesDir } from './lib/common.mjs';
import { GROUPS, buildGroupDeck, deckFile } from './lib/gen/groups.mjs';

const a = args();
if (a.flag('--list')) {
  for (const [name, g] of Object.entries(GROUPS)) console.log(`${name}\tseeds ${g.seedBase}..${g.seedBase + g.count - 1}\t${g.count} decks\t${g.ci ? 'CI (committed)' : 'local only'}`);
  process.exit(0);
}
const name = a.opt('--gen');
if (!name || !GROUPS[name]) { console.error(`usage: gen-decks.mjs --gen <${Object.keys(GROUPS).join('|')}> [--out dir] [--check] [--only i,j] [--stats]`); process.exit(2); }
const g = GROUPS[name];
const out = a.opt('--out', join(capturesDir, 'decks', name));
const only = a.opt('--only');
const indices = only ? only.split(',').map(Number) : Array.from({ length: g.count }, (_, i) => i);
const check = a.flag('--check');
if (!check) mkdirSync(out, { recursive: true });
let bytes = 0, diff = 0;
const cover = new Map();
for (const i of indices) {
  const d = buildGroupDeck(name, i);
  bytes += d.bytes.length;
  for (const f of d.features) cover.set(f, (cover.get(f) ?? 0) + 1);
  const file = join(out, deckFile(name, i));
  if (check) {
    if (!existsSync(file) || !readFileSync(file).equals(d.bytes)) { diff++; console.error(`DIFFERS: ${file}`); }
  } else writeFileSync(file, d.bytes);
}
console.log(`${name}: ${indices.length} decks, ${(bytes / 1024).toFixed(0)} KiB${check ? `, ${diff} differ from ${out}` : ` -> ${out}`}`);
if (a.flag('--stats')) for (const [f, n] of [...cover].sort()) console.log(`${String(n).padStart(5)}  ${f}`);
process.exit(diff ? 1 : 0);
