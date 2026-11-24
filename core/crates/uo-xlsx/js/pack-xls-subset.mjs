// Builds the committed `.xls` subset of the C2 parity corpus from the Excel COM outputs: each file is repacked as a fresh Compound File
// holding ONLY its `Workbook` stream (the converter reads nothing else), with the WRITEACCESS record (the user name of whoever saved)
// blanked. The Workbook stream is otherwise Excel's, byte for byte. Refuses a file in which the user name still appears.
//
//   node core/crates/uo-xlsx/js/pack-xls-subset.mjs --out <folder> [--name <text to refuse>...] [--swap <from:to, same length>...] <name=file.xls>...
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { args } from './lib/common.mjs';

const a = args();
const out = a.opt('--out');
const refuse = a.argv.filter((x, i, v) => v[i - 1] === '--name');
const swaps = a.argv.filter((x, i, v) => v[i - 1] === '--swap').map((x) => x.split(':'));
const items = a.argv.filter((x, i, v) => x.includes('=') && !['--out', '--name', '--swap'].includes(v[i - 1]));
mkdirSync(out, { recursive: true });

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
      if (size < 4096) throw new Error('mini stream');
      return { name, data: Buffer.concat(chain(dir.readUInt32LE(o + 0x74)).map((s) => b.subarray(sec(s), sec(s) + ss))).subarray(0, size) };
    }
  }
  throw new Error('no Workbook stream');
}

function writeCfb(name, data) {
  const ss = 512;
  const nData = Math.ceil(data.length / ss);
  let nFat = 1;
  while ((nData + 1 + nFat) > nFat * 128) nFat++;
  const total = nData + nFat + 1;
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
  const ent = (o, nm, type, child, start, size) => {
    Buffer.from(nm, 'utf16le').copy(buf, d + o * 128); buf.writeUInt16LE(nm ? nm.length * 2 + 2 : 0, d + o * 128 + 0x40);
    buf[d + o * 128 + 0x42] = type; buf[d + o * 128 + 0x43] = 1;
    buf.writeUInt32LE(0xffffffff, d + o * 128 + 0x44); buf.writeUInt32LE(0xffffffff, d + o * 128 + 0x48); buf.writeUInt32LE(child, d + o * 128 + 0x4c);
    buf.writeUInt32LE(start, d + o * 128 + 0x74); buf.writeUInt32LE(size, d + o * 128 + 0x78);
  };
  ent(0, 'Root Entry', 5, 1, 0xfffffffe, 0);
  ent(1, name, 2, 0xffffffff, 0, data.length);
  for (const o of [2, 3]) { buf.writeUInt32LE(0xffffffff, d + o * 128 + 0x44); buf.writeUInt32LE(0xffffffff, d + o * 128 + 0x48); buf.writeUInt32LE(0xffffffff, d + o * 128 + 0x4c); }
  return buf;
}

for (const it of items) {
  const [name, file] = it.split('=');
  const st = readStream(readFileSync(file));
  const d = Buffer.from(st.data);
  // WRITEACCESS (0x005C): the user name, 112 bytes of XLUnicodeString + padding: blank it
  for (let o = 0; o + 4 <= d.length;) {
    const ty = d.readUInt16LE(o), len = d.readUInt16LE(o + 2);
    if (ty === 0x005c) d.fill(0x20, o + 4, o + 4 + len);
    o += 4 + len;
  }
  // the user name where Excel put it as the author of a note (same length, 8-bit or UTF-16)
  for (const [from, to] of swaps) {
    for (const enc of ['latin1', 'utf16le']) {
      const f = Buffer.from(from, enc), t = Buffer.from(to, enc);
      for (let i = d.indexOf(f); i >= 0; i = d.indexOf(f, i + t.length)) t.copy(d, i);
    }
  }
  const hay = [d.toString('latin1'), d.toString('utf16le'), d.subarray(1).toString('utf16le')].join('\n').toLowerCase();
  for (const n of refuse) if (hay.includes(n.toLowerCase())) throw new Error(`${file}: the name "${n}" is still in the stream`);
  writeFileSync(join(out, `${name}.xls`), writeCfb(st.name, d.length < 4096 ? Buffer.concat([d, Buffer.alloc(4096 - d.length)]) : d));
}
console.log(`${items.length} files -> ${out}`);
