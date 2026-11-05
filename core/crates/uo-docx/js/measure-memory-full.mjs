// M3 reference of the D1a whole-document read: the heap the frozen TS `readFullDocument` keeps, for the
// documents tests/memory_r5.rs writes (`UO_M3_DIR=<dir> cargo test -p uo-docx --all-features --test
// memory_r5 -- --nocapture`), per class: `heapUsed` after a full GC with the document alive minus before
// (median of 5), the XML text the read parses, and the same with `{ sources: true }`.
//
//   UO_TS_ROOT=<built checkout of main> node --expose-gc core/crates/uo-docx/js/measure-memory-full.mjs <dir>
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { ts } from './capture-r2-common.mjs';

if (typeof globalThis.gc !== 'function') throw new Error('run with node --expose-gc');
const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const dir = process.argv[2];
const heap = () => { globalThis.gc(); globalThis.gc(); return process.memoryUsage().heapUsed; };
const median = (a) => a.slice().sort((x, y) => x - y)[a.length >> 1];

for (const file of readdirSync(dir).filter((f) => f.endsWith('.docx')).sort()) {
  const pkg = DocxPackage.load(new Uint8Array(readFileSync(join(dir, file))));
  const xml = pkg.partNames().filter((n) => n.endsWith('.xml') || n.endsWith('.rels')).reduce((s, n) => s + pkg.getPart(n).length, 0);
  const measure = (sources) => {
    readFullDocument(pkg, { sources });
    const runs = [];
    const keep = [];
    for (let i = 0; i < 5; i++) {
      keep.length = 0;
      const before = heap();
      keep.push(readFullDocument(pkg, { sources }));
      const after = heap();
      runs.push(after - before);
      if (keep[0] === undefined) throw new Error('unreachable');
    }
    return median(runs);
  };
  const plain = measure(false), src = measure(true);
  console.log(`${file.replace('.docx', '')} | xml ${xml} | TS heap ${plain} | TS heap with sources ${src}`);
}
