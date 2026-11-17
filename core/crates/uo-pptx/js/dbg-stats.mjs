// Debug helper (B4): which item kinds the Rust lists of a .uolf directory contain (coverage of the parity gate).
//   node dbg-stats.mjs <uolf dir>...
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { decodeSlidesList, newTables } from '../../uo-wasm/js/gen/slides.ts';

const ops = new Map(), eff = new Map(), paints = new Map();
let lists = 0;
for (const dir of process.argv.slice(2)) {
  for (const f of readdirSync(dir).filter((n) => n.endsWith('.uolf'))) {
    const buf = readFileSync(join(dir, f));
    const tables = newTables();
    let p = 5;
    const u32 = () => { const x = buf.readUInt32LE(p); p += 4; return x; };
    while (p < buf.length) {
      const n = u32();
      const msg = new Uint8Array(buf.subarray(p, p + n)); p += n;
      const nb = u32(); for (let i = 0; i < nb; i++) { const l = u32(); p += l; }
      if (!n) continue;
      const list = decodeSlidesList(msg, tables);
      lists++;
      for (const it of list.items) ops.set(it.op, (ops.get(it.op) ?? 0) + 1);
      for (const e of list.effects) eff.set(e.kind, (eff.get(e.kind) ?? 0) + 1);
      for (const r of list.runs) { const k = `text:${r.fill.kind}${r.bullet ? ':bullet' : ''}${r.shadow ? ':shadow' : ''}${r.outline ? ':outline' : ''}${r.underline ? ':ul' : ''}${r.strike ? ':st' : ''}`; paints.set(k, (paints.get(k) ?? 0) + 1); }
      for (const pt of list.patterns) { const k = `pattern:${pt.src.kind}${pt.tile ? ':tile' : ''}`; paints.set(k, (paints.get(k) ?? 0) + 1); }
      for (const it of list.items) if (it.op === 'fillStyle') { const k = `fill:${it.kind}`; paints.set(k, (paints.get(k) ?? 0) + 1); }
    }
  }
}
console.log('lists', lists);
console.log([...ops].sort((a, b) => b[1] - a[1]).map(([k, v]) => `${k}=${v}`).join(' '));
console.log('effects', [...eff].map(([k, v]) => `${k}=${v}`).join(' ') || '(none)');
console.log([...paints].sort().map(([k, v]) => `${k}=${v}`).join(' '));
