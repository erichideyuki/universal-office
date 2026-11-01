// Generator of the rows of the constant CASES in tests/zip_fflate.rs.
//
// Run: node tests/fflate_support/zip_fflate.mjs <fflate location>
//   e.g. <fflate location> = a UniversalOffice checkout with write/node_modules installed.
// Prints one Rust tuple per line: (label, archive hex, slides ratio filter, fflate `unzipSync`
// result as Ok(&[(name, content hex), ...]) in Object.entries order, or Err(message)).

import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

// Loads fflate from the directory given as the first argument: the fflate package itself, a
// directory with node_modules/fflate, or a checkout whose write/node_modules has it.
async function loadFflate(arg) {
  if (!arg) {
    console.error('usage: node <script> <fflate package dir | dir with node_modules | UniversalOffice checkout>');
    process.exit(2);
  }
  const base = resolve(arg);
  for (const dir of [base, join(base, 'node_modules', 'fflate'), join(base, 'write', 'node_modules', 'fflate')]) {
    const entry = join(dir, 'esm', 'index.mjs');
    if (existsSync(entry)) {
      const { version } = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
      if (version !== '0.8.3') console.error(`warning: fflate ${version}; the fixtures were made with 0.8.3`);
      return import(pathToFileURL(entry).href);
    }
  }
  console.error(`fflate not found under ${base}`);
  process.exit(2);
}
const f = await loadFflate(process.argv[2]);
const hex = (u) => Buffer.from(u).toString('hex');
const enc = (s) => new TextEncoder().encode(s);
const u16 = (v) => [v & 255, (v >>> 8) & 255];
const u32 = (v) => [v & 255, (v >>> 8) & 255, (v >>> 16) & 255, (v >>> 24) & 255];
const u64 = (v) => [...u32(v % 4294967296), ...u32(Math.floor(v / 4294967296))];
// entries: {name: bytes, flags, method, data, su, sc?, csz?, z64?: {su,sc,off}}
function build(entries, { comment = [], zip64 = false, count = null } = {}) {
  const out = []; const cen = [];
  for (const e of entries) {
    const off = out.length;
    out.push(...u32(0x04034b50), 20, 0, ...u16(e.flags || 0), ...u16(e.method), 0, 0, 0x21, 0x28, ...u32(0), ...u32(e.data.length), ...u32(e.su), ...u16(e.name.length), 0, 0, ...e.name, ...e.data);
    const ex = e.z64 ? [1, 0, ...u16(e.z64.length * 8), ...e.z64.flatMap(u64)] : [];
    const sc = e.sc ?? e.data.length; const offv = e.offv ?? off;
    cen.push(...u32(0x02014b50), 20, 0, 20, 0, ...u16(e.flags || 0), ...u16(e.method), 0, 0, 0x21, 0x28, ...u32(e.crc || 0), ...u32(sc), ...u32(e.su), ...u16(e.name.length), ...u16(ex.length), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, ...u32(offv), ...e.name, ...ex);
  }
  const cdo = out.length; out.push(...cen); const n = count ?? entries.length;
  if (zip64) { const rec = out.length; out.push(...u32(0x06064b50), ...u64(44), 45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0, ...u64(n), ...u64(n), ...u64(cen.length), ...u64(cdo)); out.push(...u32(0x07064b50), 0, 0, 0, 0, ...u64(rec), 1, 0, 0, 0); }
  out.push(...u32(0x06054b50), 0, 0, 0, 0, ...u16(zip64 ? 0xffff : n), ...u16(zip64 ? 0xffff : n), ...u32(cen.length), ...u32(zip64 ? 0xffffffff : cdo), ...u16(comment.length), ...comment);
  return new Uint8Array(out);
}
const st = (name, s, flags = 0) => ({ name: typeof name == 'string' ? enc(name) : name, flags, method: 0, data: enc(s), su: enc(s).length });
const df = (name, s, su) => { const d = f.deflateSync(enc(s)); return { name: enc(name), method: 8, data: d, su: su ?? enc(s).length }; };
const BS = String.fromCharCode(92); const rs = (s) => '"' + [...s].map(c => { const cp = c.codePointAt(0); return cp < 0x20 || cp > 0x7e || c == '"' || c == BS ? BS + 'u{' + cp.toString(16) + '}' : c; }).join('') + '"';
// the "zipsync" archive was first made with the clock time (DOS 0x5d61032f); pinned so the row
// regenerates byte for byte (a local Date: fflate writes local fields, so any time zone works)
const ZIPSYNC_MTIME = new Date(2026, 10, 1, 0, 25, 30);
const cases = [];
const run = (label, z, filter) => {
  let res; try { const u = f.unzipSync(z, filter ? { filter } : undefined); res = 'Ok(&[' + Object.entries(u).map(([k, v]) => `(${rs(k)}, "${hex(v)}")`).join(', ') + '])'; } catch (e) { res = 'Err(' + rs(e.message) + ')'; }
  cases.push(`    (${rs(label)}, "${hex(z)}", ${filter ? 'true' : 'false'}, ${res}),`);
};
run('zipsync', f.zipSync({ '[Content_Types].xml': enc('<T/>'), 'word/document.xml': enc('<w:document/>'), 'é/ü.xml': enc('x'), '10': enc('ten'), '2': enc('two'), 'dir/': new Uint8Array(0), '01': enc('z') }, { level: 6, mtime: ZIPSYNC_MTIME }));
run('duplicates', build([st('a', 'first'), st('b', 'bee'), st('a', 'second')]));
run('names', build([st([0xe9, 0x2e, 0x78], 'latin1'), st([0xff, 0x41], 'bad utf8', 0x800), st([0xef, 0xbb, 0xbf, 0x42], 'bom', 0x800), st([0xc3, 0xa9], 'utf8', 0x800), st([0xc3, 0xa9], 'latin1 pair')]));
run('proto', build([st('__proto__', 'p'), st('x', 'y')]));
run('method12', build([{ ...st('a', 'x'), method: 12 }]));
run('ratio', build([{ ...st('big', 'tiny'), su: 10 << 20 }, st('ok', 'fine'), { ...df('d', 'abcabcabc'), su: 1 << 20 }]), (e) => e.originalSize <= Math.max(1 << 20, e.size * 1100));
run('ratio_off', build([{ ...st('big', 'tiny'), su: 10 << 20 }, st('ok', 'fine')]));
run('lying_sizes', build([df('short', 'abcdefghij'.repeat(5), 7), df('long', 'abcdefghij', 100), { ...df('empty', ''), data: new Uint8Array(0), su: 4 }]));
run('bad_deflate', build([st('a', 'x'), { ...df('b', 'x'), data: new Uint8Array([7]) }]));
const good = build([st('a', 'hello'), df('b', 'world world world')]);
run('truncated', good.subarray(0, good.length - 30));
run('no_eocd', new Uint8Array(100));
run('tiny', new Uint8Array([0x50, 0x4b, 5, 6]));
run('count0', build([], {}));
run('comment', build([st('a', 'hello')], { comment: [...enc('c'.repeat(300))] }));
run('zip64', build([{ ...st('a', 'z64 data'), su: 0xffffffff, sc: 0xffffffff, z64: [8, 8] }, st('b', 'plain')], { zip64: true }));
run('zip64_missing_extra', build([{ ...st('a', 'z64 data'), su: 0xffffffff }], { zip64: true }));
run('not_zip64_ff', build([{ ...st('a', 'data'), sc: 0xffffffff }]));
run('range_beyond', build([{ ...st('a', 'abc'), sc: 1000 }]));
run('stored_ignores_su', build([{ ...st('a', 'abcdef'), su: 2 }]));
run('dir_entries', build([st('d/', ''), st('d/f', 'f')]));
console.log(cases.join('\n'));
