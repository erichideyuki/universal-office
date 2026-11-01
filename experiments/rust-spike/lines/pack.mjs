// Converte a captura (data/lines.json) para o formato binário que o Rust lê (igual no nativo e no
// WebAssembly, sem biblioteca de JSON): little-endian, registros de tamanho fixo.
// As cadeias viram códigos e os testes de caractere que a quebra faz viram bits:
// letra (\p{L} sem \p{M}, a regra das candidatas de hifenização), espaço comum, vírgula/ponto.
//   node pack.mjs   →   ../data/lines.bin
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const d = JSON.parse(readFileSync(join(here, '../data/lines.json'), 'utf8'));

const KIND = ['char', 'space', 'nbsp', 'tab', 'lb', 'pb', 'cb', 'obj', 'shy', 'box'];
const LEADER = [null, 'dot', 'hyphen', 'underscore', 'heavy', 'middleDot'];
const TABVAL = { left: 0, center: 1, right: 2, decimal: 3, bar: 4, clear: 5 };
const JC = { left: 0, center: 1, right: 2, both: 3, distribute: 4 };
const RULE = { auto: 0, exact: 1, atLeast: 2 };
const BRK = { page: 1, column: 2 };
const GAP = { break: 1, start: 2 };
const leader = (l) => { const i = LEADER.indexOf(l === 'none' ? null : l); if (i < 0) throw new Error(`leader ${l}`); return i; };
const LETTER = /^[\p{L}\p{M}]$/u, MARK = /^\p{M}$/u;

const chunks = [];
let buf = Buffer.alloc(1 << 20), at = 0;
const room = (n) => { if (at + n > buf.length) { chunks.push(buf.subarray(0, at)); buf = Buffer.alloc(Math.max(1 << 20, n)); at = 0; } };
const u8 = (v) => { room(1); buf.writeUInt8(v, at); at += 1; };
const u32 = (v) => { room(4); buf.writeUInt32LE(v >>> 0, at); at += 4; };
const i32 = (v) => { room(4); buf.writeInt32LE(v, at); at += 4; };
const f64 = (v) => { room(8); buf.writeDoubleLE(v ?? NaN, at); at += 8; };

// glifo: 1 tipo + 2 flags + 4 cp + 12 f64 + 2 (ptab)
const glyph = (g) => {
  const k = KIND.indexOf(g.k);
  if (k < 0) throw new Error(`kind ${g.k}`);
  const flags = (g.br ? 1 : 0) | (g.glue ? 2 : 0) | (g.clear ? 4 : 0) | (g.hr !== undefined ? 8 : 0) | (g.ptab ? 16 : 0) | (g.fixed ? 32 : 0)
    | (g.hang ? 64 : 0) | (g.hyph ? 128 : 0) | (g.ul ? 256 : 0) | (g.k === 'char' && LETTER.test(g.ch) && !MARK.test(g.ch) ? 512 : 0)
    | (g.ch === ' ' ? 1024 : 0) | (g.ch === ',' || g.ch === '.' ? 2048 : 0) | (g.nAsc !== undefined ? 4096 : 0) | (g.nDesc !== undefined ? 8192 : 0)
    | (g.fe !== undefined ? 16384 : 0);
  u8(k); u8(flags & 0xff); u8(flags >> 8); u32(g.cp);
  for (const v of [g.adv, g.asc, g.desc, g.nAsc, g.nDesc, g.hr, g.fe, g.hyW, g.fa, g.fd, g.sz, 0]) f64(v);
  u8(g.ptab ? ({ left: 0, center: 1, right: 2 })[g.ptab.a] : 0); u8(g.ptab ? leader(g.ptab.l) : 0);
};

u32(0x31534c55); // "ULS1"
f64(d.opts.justifyShrink.ratio); f64(d.opts.justifyShrink.expandRatio);
u32(d.paras.length);
for (const p of d.paras) {
  const s = p.settings;
  f64(s.defaultTabStop); i32(s.compatibilityMode); u8(s.doNotUseIndentAsNumberingTabStop ? 1 : 0); u8(s.splitPgBreakAndParaMark ? 1 : 0);
  i32(s.consecutiveHyphenLimit); f64(s.hyphenationZone);
  f64(p.grid);
  f64(p.firstLine); f64(p.indLeft); f64(p.indRight); u8(JC[p.jc] ?? 0); u8(RULE[p.lineRule]); f64(p.line);
  f64(p.markAsc); f64(p.markDesc);
  const ms = p.markStyle;
  f64(ms.sizePt); f64(ms.shift); u8(ms.vertAlign === 'superscript' ? 1 : ms.vertAlign === 'subscript' ? 2 : 0); f64(ms.sz); f64(ms.shadow[0]); f64(ms.shadow[1]);
  u32(p.tabs.length);
  for (const t of p.tabs) { u8(TABVAL[t.val] ?? 0); f64(t.pos); u8(leader(t.leader)); }
  u32(p.glyphs.length);
  for (const g of p.glyphs) glyph(g);
  u8(p.marker ? 1 : 0);
  if (p.marker) {
    const m = p.marker;
    u8(m.jc === 'right' ? 2 : m.jc === 'center' ? 1 : 0); f64(m.width); u8(m.suff === 'tab' ? 1 : m.suff === 'space' ? 2 : 0); f64(m.spaceW);
    u32(m.glyphs.length);
    for (const g of m.glyphs) glyph(g);
  }
}
u32(d.events.length);
for (const e of d.events) {
  if (e[0] === 'new') { u8(0); u32(e[1]); }
  else if (e[0] === 'next') {
    const f = e[2];
    u8(1); u32(e[1]); f64(f.left); f64(f.right); u8(f.gap ? 1 : 0); u8(f.next ? 1 : 0); f64(f.next?.left); f64(f.next?.right);
  } else { const m = e[2]; u8(2); u32(e[1]); u32(m.pos); u32(m.lineIdx); u8(m.trailingBreak ? 1 : 0); u32(m.hyphenRun); }
}
u32(d.lines.length);
const placed = (list) => { u32(list.length); for (const q of list) { i32(q[0]); f64(q[1]); f64(q[2]); u8(leader(q[3])); } };
for (const l of d.lines) {
  u32(l.start); u32(l.end);
  for (const v of [l.width, l.xStart, l.asc, l.desc, l.height, l.baseline]) f64(v);
  u8(l.last ? 1 : 0); u8(BRK[l.breakAfter] ?? 0); u8(l.clear ? 1 : 0); u8(GAP[l.gapTab] ?? 0); u8(l.spanned ? 1 : 0); u8(l.hr ? 1 : 0);
  placed(l.placed);
  u8(l.marker ? 1 : 0);
  if (l.marker) placed(l.marker);
}
chunks.push(buf.subarray(0, at));
const out = Buffer.concat(chunks);
writeFileSync(join(here, '../data/lines.bin'), out);
console.log(`lines.bin: ${(out.length / 1048576).toFixed(1)} MB`);
