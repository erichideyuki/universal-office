// Runs the wasm consumer in Node: the 10,000 × 40 heat map (a styled cell each, groups of 50,
// a band and a header row), timed, written to <out>.xlsx for the Excel open check.
//   node core/crates/uo-xlsx/wasm-consumer/smoke.mjs <module.wasm> <out.xlsx> [rows] [cols]
import { readFileSync, writeFileSync } from 'node:fs';

const [file, out, rowsArg, colsArg] = process.argv.slice(2);
const rows = Number(rowsArg ?? 10000), cols = Number(colsArg ?? 40);
const { instance } = await WebAssembly.instantiate(readFileSync(file), {});
const x = instance.exports;
const enc = new TextEncoder();

class Buf {
  constructor() { this.a = []; }
  u8(v) { this.a.push(v & 255); return this; }
  u32(v) { this.a.push(v & 255, (v >>> 8) & 255, (v >>> 16) & 255, (v >>> 24) & 255); return this; }
  f64(v) { const b = new Uint8Array(new Float64Array([v]).buffer); this.a.push(...b); return this; }
  str(s) { const b = enc.encode(s); this.u32(b.length); this.a.push(...b); return this; }
  bytes() { return Uint8Array.from(this.a); }
}
function call(fn, buf) {
  const b = buf.bytes();
  const p = x.alloc(b.length);
  new Uint8Array(x.memory.buffer, p, b.length).set(b);
  return fn(p, b.length);
}

const t0 = performance.now();
if (!call(x.create, new Buf().str('Mapa de calor').str('pt-BR'))) throw new Error('create');
const style = (flags, color, fill, align, fmt) => call(x.add_style, new Buf().u8(flags).u32(color).u32(fill).u8(align).str(fmt));
const header = style(1 | 16 | 32, 0, 0, 2, '');
const band = style(1, 0, 0, 6, '');
const group = style(1 | 8, 0, 0xeeeeee, 0, '');
const scale = Array.from({ length: 10 }, (_, k) => style(8 | (k > 6 ? 4 : 0), 0xffffff, (255 << 16) | ((255 - k * 20) << 8) | (255 - k * 20), 2, '0%'));
const sheet = new Buf().str('Mapa de calor').u8(0).u32(2).u8(1).f64(28);
for (let c = 0; c < cols; c++) sheet.f64(9);
if (!call(x.begin_sheet, sheet)) throw new Error('begin_sheet');
const bandRow = new Buf().u8(0).u8(0).u32(0);
for (let c = 0; c < cols; c++) c % 10 === 0 ? bandRow.u8(1).u32(band).str(['Políticas', 'Treinamentos', 'Contratos', 'Normas'][(c / 10) % 4]) : bandRow.u8(0).u32(band);
call(x.append_row, bandRow);
const head = new Buf().u8(0).u8(1).u32(header).str('Usuário');
for (let c = 0; c < cols; c++) head.u8(1).u32(header).str(`Documento ${String(c + 1).padStart(2, '0')}`);
call(x.append_row, head);
let seed = 0x12345678;
for (let i = 0; i < rows; i++) {
  if (i % 50 === 0) call(x.append_row, new Buf().u8(0).u8(1).u32(group).str(`Departamento: ${i / 50 + 1} (${Math.min(50, rows - i)})`));
  const r = new Buf().u8(1).u8(0x81).u32(0).str(`Usuário ${String(i + 1).padStart(5, '0')}`).str(`mailto:usuario${i}@example.com`);
  for (let c = 0; c < cols; c++) {
    seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0;
    const v = (seed % 101) / 100;
    r.u8(2).u32(scale[Math.min(9, Math.floor(v * 9.99))]).f64(v);
  }
  if (!call(x.append_row, r)) throw new Error(`row ${i}`);
}
if (!x.finish()) throw new Error('finish');
const bytes = new Uint8Array(x.memory.buffer, x.out_ptr(), x.out_len()).slice();
const ms = performance.now() - t0;
writeFileSync(out, bytes);
console.log(`${rows} x ${cols}: ${bytes.length} bytes in ${ms.toFixed(0)} ms (wasm, including the JS encoding); memory ${(x.memory.buffer.byteLength / 1e6).toFixed(1)} MB`);
