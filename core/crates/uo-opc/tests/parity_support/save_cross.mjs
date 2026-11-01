// Cross reading of saved packages (opt-in part of tests/opc_parity.rs, `save_cross_readers`):
// for every line `<index>\t<app w|s|p>\t<input path>` of <dir>/inputs.tsv, the TS app loads the
// input and saves it (<dir>/<index>.ts.zip); then fflate reads back that archive and the two the
// Rust package wrote (<index>.raw.zip: untouched entries copied raw; <index>.rw.zip: every part
// rewritten). Prints `<file>\t<value>` with the value of the `save.*` facet (count, digest with
// the method, digest without it) as fflate sees each archive.
//
//   node save_cross.mjs <dir>        (UO_TS_ROOT = a built checkout of main)
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { Stream, importFrom, tsRoot } from '../../../uo-testkit/js/testkit.mjs';

const dir = process.argv[2];
const root = tsRoot(process.argv.slice(3));
const { DocxPackage } = await importFrom(root, 'write/packages/docx-core/dist/package.js');
const { XlsxPackage } = await importFrom(root, 'sheet/packages/sheet-core/dist/package.js');
const { PptxPackage } = await importFrom(root, 'slides/packages/slides-core/dist/package.js');
const fflate = await importFrom(root, 'write/node_modules/fflate/esm/index.mjs');
const load = { w: (b) => DocxPackage.load(b), s: (b) => XlsxPackage.load(b), p: (b) => PptxPackage.load(b) };

function centralDirectory(z) {
  const dv = new DataView(z.buffer, z.byteOffset, z.byteLength);
  let e = z.length - 22;
  while (dv.getUint32(e, true) !== 0x06054b50) e--;
  const count = dv.getUint16(e + 10, true);
  let at = dv.getUint32(e + 16, true);
  const list = [];
  for (let i = 0; i < count; i++) {
    const nl = dv.getUint16(at + 28, true), xl = dv.getUint16(at + 30, true), cl = dv.getUint16(at + 32, true);
    list.push({ name: z.subarray(at + 46, at + 46 + nl), flags: dv.getUint16(at + 8, true), method: dv.getUint16(at + 10, true) });
    at += 46 + nl + xl + cl;
  }
  return list;
}
const decodeName = (u8, flags) => (flags & 0x800 ? new TextDecoder().decode(u8) : String.fromCharCode(...u8));

function readBack(z) {
  const back = fflate.unzipSync(z);
  const full = new Stream(), noMethod = new Stream();
  const list = centralDirectory(z);
  for (const c of list) {
    const data = back[decodeName(c.name, c.flags)];
    if (data === undefined) throw new Error('entry not read back');
    full.bytes(c.name).u32(c.flags & 0x800).u32(c.method).digestOf(data);
    noMethod.bytes(c.name).u32(c.flags & 0x800).digestOf(data);
  }
  return `${list.length}:${full.finish().short}:${noMethod.finish().short}`;
}

const out = [];
for (const line of readFileSync(join(dir, 'inputs.tsv'), 'utf8').split('\n')) {
  if (!line) continue;
  const [i, app, path] = line.split('\t');
  const saved = load[app](new Uint8Array(readFileSync(path))).save();
  writeFileSync(join(dir, `${i}.ts.zip`), saved);
  for (const kind of ['ts', 'raw', 'rw']) {
    const f = join(dir, `${i}.${kind}.zip`);
    if (!existsSync(f)) continue;
    let value;
    try { value = readBack(new Uint8Array(readFileSync(f))); } catch (e) { value = `err:${e.message}`; }
    out.push(`${i}.${kind}.zip\t${value}`);
  }
}
process.stdout.write(out.join('\n') + '\n');
