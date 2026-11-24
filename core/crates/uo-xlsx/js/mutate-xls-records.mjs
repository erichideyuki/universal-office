// Record-level seeded mutants of `.xls` files (frontier C2): the Workbook stream is read out of the Compound File, its BIFF records are
// mutated (data bytes flipped, a record deleted, duplicated, swapped, truncated, retyped, inserted) and the stream is written back in a
// fresh minimal Compound File — so, unlike the byte-level mutants of `mutate-xls.mjs`, the container stays valid and the mutation
// reaches the converter's logic (formulas, shared formulas, drawings, charts, notes, conditional formats…).
//
//   node core/crates/uo-xlsx/js/mutate-xls-records.mjs --out <folder> --per <n> [--seed <s>] [--max <bytes>] <folder|file>...
import { mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { args } from './lib/common.mjs';

const a = args();
const out = a.opt('--out');
const per = Number(a.opt('--per', 8));
let seed = Number(a.opt('--seed', 1)) >>> 0 || 1;
const max = Number(a.opt('--max', 600_000));
const flagsWithValue = ['--out', '--per', '--seed', '--max'];
const inputs = a.argv.filter((x, i, v) => !x.startsWith('--') && !flagsWithValue.includes(v[i - 1]));
const rnd = () => { seed ^= seed << 13; seed >>>= 0; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0; return seed / 4294967296; };
const int = (n) => Math.floor(rnd() * n);

/** The "Workbook" (or "Book") stream of a Compound File written by Excel (regular streams only). */
function readStream(b) {
  const u32 = (o) => b.readUInt32LE(o);
  const ss = 1 << b.readUInt16LE(0x1e);
  const sec = (n) => (n + 1) * ss;
  const fatSecs = [];
  for (let i = 0; i < 109; i++) { const s = u32(0x4c + i * 4); if (s < 0xfffffffa) fatSecs.push(s); }
  let difat = u32(0x44);
  for (let g = 0; g < u32(0x48) && difat < 0xfffffffa; g++) { for (let i = 0; i < ss / 4 - 1; i++) { const s = u32(sec(difat) + i * 4); if (s < 0xfffffffa) fatSecs.push(s); } difat = u32(sec(difat) + ss - 4); }
  const fat = [];
  for (const s of fatSecs) for (let i = 0; i < ss / 4; i++) fat.push(u32(sec(s) + i * 4));
  const chain = (start) => { const o = []; let s = start; while (s < 0xfffffffa && o.length < 1e6) { o.push(s); s = fat[s] ?? 0xfffffffe; } return o; };
  const dir = Buffer.concat(chain(u32(0x30)).map((s) => b.subarray(sec(s), sec(s) + ss)));
  for (let o = 0; o + 128 <= dir.length; o += 128) {
    const len = dir.readUInt16LE(o + 0x40);
    const name = dir.subarray(o, o + Math.max(0, len - 2)).toString('utf16le');
    if (dir[o + 0x42] === 2 && (name === 'Workbook' || name === 'Book')) {
      const size = dir.readUInt32LE(o + 0x78);
      if (size < 4096) return undefined;
      return { name, data: Buffer.concat(chain(dir.readUInt32LE(o + 0x74)).map((s) => b.subarray(sec(s), sec(s) + ss))).subarray(0, size) };
    }
  }
  return undefined;
}

/** A minimal Compound File v3 with one stream (>= 4096 bytes: no mini stream). */
function writeCfb(name, data) {
  const ss = 512;
  const nData = Math.ceil(data.length / ss);
  let nFat = 1;
  while ((nData + 1 + nFat) > nFat * 128) nFat++;
  const total = nData + nFat + 1; // data, FAT, directory
  const buf = Buffer.alloc(ss * (total + 1));
  Buffer.from([0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]).copy(buf, 0);
  buf.writeUInt16LE(0x3e, 0x18); buf.writeUInt16LE(3, 0x1a); buf.writeUInt16LE(0xfffe, 0x1c); buf.writeUInt16LE(9, 0x1e); buf.writeUInt16LE(6, 0x20);
  buf.writeUInt32LE(nFat, 0x2c); buf.writeUInt32LE(nData + nFat, 0x30); buf.writeUInt32LE(4096, 0x38);
  buf.writeUInt32LE(0xfffffffe, 0x3c); buf.writeUInt32LE(0, 0x40); buf.writeUInt32LE(0xfffffffe, 0x44); buf.writeUInt32LE(0, 0x48);
  for (let i = 0; i < 109; i++) buf.writeUInt32LE(i < nFat ? nData + i : 0xffffffff, 0x4c + i * 4);
  data.copy(buf, ss);
  const fat = new Array(nFat * 128).fill(0xffffffff);
  for (let i = 0; i < nData; i++) fat[i] = i + 1 < nData ? i + 1 : 0xfffffffe;
  for (let i = 0; i < nFat; i++) fat[nData + i] = 0xfffffffd;
  fat[nData + nFat] = 0xfffffffe;
  fat.forEach((v, i) => buf.writeUInt32LE(v, ss * (1 + nData) + i * 4));
  const d = ss * (1 + nData + nFat);
  const ent = (o, nm, type, left, right, child, start, size) => {
    Buffer.from(nm, 'utf16le').copy(buf, d + o * 128); buf.writeUInt16LE(nm ? nm.length * 2 + 2 : 0, d + o * 128 + 0x40);
    buf[d + o * 128 + 0x42] = type; buf[d + o * 128 + 0x43] = 1;
    buf.writeUInt32LE(left, d + o * 128 + 0x44); buf.writeUInt32LE(right, d + o * 128 + 0x48); buf.writeUInt32LE(child, d + o * 128 + 0x4c);
    buf.writeUInt32LE(start, d + o * 128 + 0x74); buf.writeUInt32LE(size, d + o * 128 + 0x78);
  };
  ent(0, 'Root Entry', 5, 0xffffffff, 0xffffffff, 1, 0xfffffffe, 0);
  ent(1, name, 2, 0xffffffff, 0xffffffff, 0xffffffff, 0, data.length);
  for (const o of [2, 3]) { buf.writeUInt32LE(0xffffffff, d + o * 128 + 0x44); buf.writeUInt32LE(0xffffffff, d + o * 128 + 0x48); buf.writeUInt32LE(0xffffffff, d + o * 128 + 0x4c); }
  return buf;
}

function records(d) {
  const r = [];
  for (let o = 0; o + 4 <= d.length;) { const len = d.readUInt16LE(o + 2); r.push(d.subarray(o, Math.min(d.length, o + 4 + len))); o += 4 + len; }
  return r;
}

const TYPES = [0x0006, 0x000a, 0x001c, 0x0017, 0x0018, 0x04bc, 0x0221, 0x01b0, 0x01b1, 0x0879, 0x087a, 0x087b, 0x01be, 0x009d, 0x01ae, 0x0031, 0x003c, 0x003d, 0x0041, 0x005d, 0x007d, 0x0085, 0x00bd, 0x00be, 0x00d6, 0x00e0, 0x00e5, 0x00eb, 0x00ec, 0x00fd, 0x00fc, 0x01b6, 0x01b8, 0x0200, 0x0201, 0x0203, 0x0204, 0x0205, 0x0207, 0x0208, 0x0225, 0x023e, 0x027e, 0x041e, 0x0800, 0x0809, 0x087d, 0x0896, 0x0236, 0x1051, 0x1002, 0x1003, 0x1006, 0x1025, 0x1033, 0x1034, 0x0812, 0x0023, 0x0092, 0x0055, 0x0099, 0x00a0];
function mutate(recs) {
  const r = recs.map((x) => Buffer.from(x));
  const ops = 1 + int(3);
  for (let k = 0; k < ops; k++) {
    const i = int(r.length);
    const kind = int(8);
    if (kind === 0 || kind === 1) { const x = r[i]; if (x.length > 4) for (let j = 0; j < 1 + int(3); j++) x[4 + int(x.length - 4)] ^= 1 << int(8); }
    else if (kind === 2) r.splice(i, 1);
    else if (kind === 3) r.splice(i, 0, Buffer.from(r[i]));
    else if (kind === 4 && i + 1 < r.length) { const t = r[i]; r[i] = r[i + 1]; r[i + 1] = t; }
    else if (kind === 5) { const x = r[i]; if (x.length > 5) { const n = int(x.length - 4); const y = Buffer.from(x.subarray(0, 4 + n)); y.writeUInt16LE(n, 2); r[i] = y; } }
    else if (kind === 6) { r[i].writeUInt16LE(TYPES[int(TYPES.length)], 0); }
    else { const n = int(40); const y = Buffer.alloc(4 + n); y.writeUInt16LE(TYPES[int(TYPES.length)], 0); y.writeUInt16LE(n, 2); for (let j = 0; j < n; j++) y[4 + j] = int(256); r.splice(i, 0, y); }
  }
  return Buffer.concat(r);
}

mkdirSync(out, { recursive: true });
const files = [];
for (const p of inputs) {
  if (statSync(p).isDirectory()) for (const f of readdirSync(p).sort()) { if (/\.xls$/i.test(f)) files.push(join(p, f)); }
  else files.push(p);
}
let n = 0, skipped = 0;
for (const f of files) {
  const src = readFileSync(f);
  if (src.length > max) { skipped++; continue; }
  const st = readStream(src);
  if (!st) { skipped++; continue; }
  const recs = records(st.data);
  const name = basename(f, '.xls');
  for (let k = 0; k < per; k++) {
    const data = mutate(recs);
    writeFileSync(join(out, `${name}-r${k}.xls`), writeCfb(st.name, data.length < 4096 ? Buffer.concat([data, Buffer.alloc(4096 - data.length)]) : data));
    n++;
  }
}
console.log(`${n} record mutants of ${files.length - skipped} files (${skipped} skipped) -> ${out}`);
