// Debug helper of the D5b parity: prints one part of the .odt the frozen TS writes for a docx (or a synthetic
// docx of tests/parity/*.inputs), or the model of readOdt of a generated .odt, so a mismatch can be diffed against
// the Rust side (`UO_ODF_DUMP`, tests/parity_odf.rs).
//
//   UO_TS_ROOT=... node js/odf-dump.mjs docx <file.docx> <part>                      a part (content.xml, styles.xml, ...)
//   UO_TS_ROOT=... node js/odf-dump.mjs docx <inputs-file> <key> <part>              the same for a synthetic docx
//   UO_TS_ROOT=... node js/odf-dump.mjs trace <key>                                 readable canonical trace of readOdt (diff with Rust's)
//   UO_TS_ROOT=... node js/odf-dump.mjs odt <key>                                    JSON dump of readOdt of tests/parity/odf.inputs
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { importFrom } from '../../uo-testkit/js/testkit.mjs';
import { crate, ts, root, readBytes } from './capture-r2-common.mjs';
import { digestOdt } from './odf-canon.mjs';

const [mode, a, b, c] = process.argv.slice(2);
const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const odf = await ts('odf/index.js');
const fflate = await importFrom(root, 'write/node_modules/fflate/lib/index.cjs');
const unesc = (line) => line.replace(/\\(.)/g, (_, x) => ({ n: '\n', r: '\r', t: '\t' })[x] ?? x);

function inputs(file, key) {
  const lines = readFileSync(join(crate, 'tests', 'parity', file), 'utf8').split('\n').filter((l) => !l.startsWith('#'));
  const parts = [];
  let on = false;
  for (const l of lines) {
    if (l.startsWith('@input ')) { if (on) break; on = l.slice(7) === key; continue; }
    if (!on) continue;
    const i = l.indexOf('\t');
    if (i < 0) continue;
    const data = l.slice(i + 1);
    parts.push([l.slice(0, i), data.startsWith('@hex:') ? Buffer.from(data.slice(5), 'hex') : unesc(data)]);
  }
  return parts;
}

if (mode === 'docx') {
  let pkg;
  if (a.endsWith('.docx')) pkg = DocxPackage.load(readBytes(a));
  else {
    const parts = inputs(a, b);
    pkg = DocxPackage.empty();
    if (!parts.some(([p]) => p === 'word/document.xml')) pkg.removePart('word/document.xml');
    for (const [p, d] of parts) pkg.setPartText(p, d);
  }
  const bytes = odf.writeOdt(readFullDocument(pkg));
  const files = fflate.unzipSync(bytes);
  const part = a.endsWith('.docx') ? b : c;
  process.stdout.write(Buffer.from(files[part] ?? new Uint8Array()).toString('utf8'));
} else if (mode === 'trace') {
  const parts = inputs('odf.inputs', a);
  const files = {};
  for (const [n, d] of parts) files[n] = typeof d === 'string' ? new TextEncoder().encode(d) : d;
  process.stdout.write(digestOdt(odf.readOdt(odf.writeZip(files)), true));
} else if (mode === 'odt') {
  const parts = inputs('odf.inputs', a);
  const files = {};
  for (const [n, d] of parts) files[n] = typeof d === 'string' ? new TextEncoder().encode(d) : d;
  const doc = odf.readOdt(odf.writeZip(files));
  const rep = (k, v) => (v instanceof Map ? Object.fromEntries(v) : v instanceof Uint8Array ? `bytes(${v.length})` : v);
  console.log(JSON.stringify({ blocks: doc.blocks, sections: doc.sections, comments: doc.comments, styles: doc.styles }, rep, 1));
}
