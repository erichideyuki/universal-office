// PP0 for the writer layer of uo-pdf (frontier W): drives the frozen TS pdf-writer API
// (write/packages/docx-layout/src/pdf-writer/**, built to dist/) with generated drawing-op
// scripts and writes tests/parity/writer.cap, the file tests/writer_parity.rs replays.
//
//   UO_TS_ROOT=<built checkout of main> node tests/parity_support/capture_writer.mjs [--scripts N] [--dump <dir>]
//
// Deterministic: the scripts come from the xorshift32 of uo-testkit (the same generator as
// src/generate.rs), the fonts are the open fonts under shared/fonts/files, the images are
// pseudo-random bytes. Each record holds
//   script  the op script (JSON; numbers that JSON cannot hold are the strings "NaN", "Infinity",
//           "-Infinity" and "-0"; see tests/support/script.rs for the interpreter on the Rust side)
//   raw     length:sha256 of the PDF made with compress: false          (compared byte for byte)
//   norm    objects:deflated:digest of the PDF made with compress: true (compared structurally:
//           object by object the dictionary without /Filter and /Length, and the payload inflated;
//           the xref and the trailer are validated and left out; `deflated` counts the streams
//           that were deflated, where the "only if smaller" rule can flip between fflate and
//           miniz_oxide, and is reported, not required)
// and the facets of the scalar functions (fmt, hexColor, safePdfUri, utf16beHex).
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { inflateSync } from 'node:zlib';
import { CaptureWriter, Rng, Stream, importFrom, tsRoot } from '../../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const fontDir = join(here, '../../../../../shared/fonts/files');
export const FONT_FILES = ['carlito-regular.ttf', 'carlito-bold.ttf', 'liberationserif-italic.ttf', 'liberationmono-regular.ttf', 'caladea-bold.ttf'];

const argv = process.argv.slice(2);
const arg = (name, dflt) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : dflt; };
const SCRIPTS = Number(arg('--scripts', '160'));
const DUMP = arg('--dump', null);

const root = tsRoot(argv);
const W = 'write/packages/docx-layout/dist/';
const pdfw = await importFrom(root, `${W}pdf-writer/index.js`);
const docmod = await importFrom(root, `${W}pdf-writer/document.js`);
const { SfntFont } = await importFrom(root, `${W}typography/font/sfnt.js`);
const { PdfDocument, safePdfUri, hexColor, utf16beHex } = { ...pdfw, ...docmod };
const fmt = docmod.fmt;

const fontBytes = FONT_FILES.map((f) => new Uint8Array(readFileSync(join(fontDir, f))));
const fonts = fontBytes.map((b) => new SfntFont(b));

// ---- values -------------------------------------------------------------------------------

const SPECIAL = new Map([['NaN', NaN], ['Infinity', Infinity], ['-Infinity', -Infinity], ['-0', -0]]);
const dec = (x) => (typeof x === 'string' ? SPECIAL.get(x) : x);
const enc = (v) => (Number.isNaN(v) ? 'NaN' : v === Infinity ? 'Infinity' : v === -Infinity ? '-Infinity' : Object.is(v, -0) ? '-0' : v);
const unit = (r) => r.next() / 4294967296;

/** A coordinate: mostly plain, sometimes the values `fmt` is built to survive. */
function coord(r, lo = -40, hi = 700) {
  const k = r.below(100);
  if (k < 55) return lo + unit(r) * (hi - lo);
  if (k < 70) return Math.round(lo + unit(r) * (hi - lo));
  if (k < 80) return Math.round(lo + unit(r) * (hi - lo)) + [0.0005, 0.0015, 0.5, 0.25, 0.9995, 0.0004999][r.below(6)];
  if (k < 86) return [1e9, 1e9 + 1, -1e9, 1e12, -1e300, 1e300, 123456789.0004][r.below(7)];
  if (k < 92) return enc([NaN, Infinity, -Infinity, -0][r.below(4)]);
  if (k < 96) return [1e-7, -1e-7, 0.0001, 5e-324][r.below(4)];
  return lo + unit(r) * 3;
}
const jv = (v) => (typeof v === 'number' ? enc(v) : v);
const c = (r, lo, hi) => { const v = coord(r, lo, hi); return typeof v === 'number' ? enc(v) : v; };
const pos = (r, hi = 300) => { const v = unit(r) * hi; return r.below(12) === 0 ? enc([0, -0, NaN, 1e9][r.below(4)]) : v; };
const color = (r) => [0, 1, 2].map(() => (r.below(8) === 0 ? enc([0, 1, NaN, -0, 0.2 + 0.1, 255 / 255, 17 / 255][r.below(7)]) : unit(r)));
const small = (r, n) => 1 + r.below(n);

// ---- script generator ---------------------------------------------------------------------

const NAMES = ['Carlito', 'Liberation Serif-Italic', 'ABCDEF+Symbol', 'Symbol MT, Bold', 'SymbolMT-Bold', 'Wingdings', 'XYZABC+Wingdings-Bold', 'Wingdings 2', 'My Font! (x)', 'symbolic', '\u{c0}\u{c9} \u{dc}font', 'AbCdEf+Calibri', 'abcdef+Symbol', 'Symbol,MTx'];

function genPathCmds(r) {
  const cmds = [];
  const n = r.below(7);
  if (n === 0 && r.below(3) !== 0) return cmds;
  cmds.push(['M', c(r), c(r)]);
  for (let i = 0; i < n; i++) {
    const k = r.below(4);
    if (k === 0) cmds.push(['L', c(r), c(r)]);
    else if (k === 1) cmds.push(['C', c(r), c(r), c(r), c(r), c(r), c(r)]);
    else if (k === 2) cmds.push(['Z']);
    else cmds.push(['M', c(r), c(r)]);
  }
  return cmds;
}

/** Document-level resources (also made in the middle of a page, as the painter does). */
function genResource(r, st) {
  const k = r.below(10);
  if (k < 3 && st.fonts < 3) {
    st.fonts++;
    const prog = r.below(3);
    const p = prog === 0 ? ['face'] : prog === 1 ? ['sym']
      : ['custom', [1000, 2048, 1024, 0, enc(NaN)][r.below(5)], r.below(3) === 0 ? null : coord(r, 0, 2000), r.below(3) === 0 ? null : coord(r, -800, 0),
        r.below(2) === 0 ? null : [coord(r, -300, 0), coord(r, -300, 0), coord(r, 500, 2000), coord(r, 500, 2000)], r.below(2) === 0 ? null : [0, -11.5, 4.25, 0.0005][r.below(4)]];
    return ['font', r.below(FONT_FILES.length), NAMES[r.below(NAMES.length)], p];
  }
  if (k < 5 && st.images < 4) {
    st.images++;
    const comps = [1, 3, 4, 3, 2, 0][r.below(6)];
    const mask = r.below(3) === 0 ? [1 + r.below(1000), 1 + r.below(3000), 1 + r.below(40), 1 + r.below(40)] : null;
    const decode = r.below(3) === 0 ? Array.from({ length: [2, 6, 8][r.below(3)] }, () => coord(r, -1, 2)) : null;
    return ['jpeg', 1 + r.below(100000), r.below(3) === 0 ? r.below(40) : 20 + r.below(3000), 1 + r.below(400), 1 + r.below(400), comps, r.below(2), decode, mask];
  }
  if (k < 8 && st.images < 4) {
    st.images++;
    return ['rgba', 1 + r.below(100000), 1 + r.below(18), 1 + r.below(18), r.below(6)];
  }
  if (st.shadings < 3) {
    st.shadings++;
    const axial = r.below(2) === 0;
    return axial
      ? ['shading', 0, [c(r), c(r), c(r), c(r)], r.below(4) === 0 ? r.below(9) : 3 * (2 + r.below(100)) + r.below(3), 1 + r.below(100000), c(r, 100, 900)]
      : ['shading', 1, [c(r), c(r), c(r)], r.below(4) === 0 ? r.below(9) : 3 * (2 + r.below(100)) + r.below(3), 1 + r.below(100000), c(r, 100, 900)];
  }
  return null;
}

function genPageOps(r, st, depth) {
  const ops = [];
  const n = 2 + r.below(14);
  for (let i = 0; i < n; i++) {
    const k = r.below(30);
    if (k < 3) ops.push(['rect', c(r), c(r), pos(r), pos(r), color(r)]);
    else if (k < 5) ops.push(['line', c(r), c(r), c(r), c(r), color(r), pos(r, 8), r.below(3) === 0 ? [] : Array.from({ length: 1 + r.below(3) }, () => pos(r, 9))]);
    else if (k < 11 && st.fonts > 0) {
      const cps = [];
      const len = 1 + r.below(r.below(5) === 0 ? 60 : 12);
      let x = coord(r, 0, 400), base = coord(r, 20, 700);
      for (let g = 0; g < len; g++) {
        const cpPool = [0x41 + r.below(26), 0x61 + r.below(26), 0x20, 0xe9, 0x2022, 0xf0b7 - 0xf000 + 0xf000, 0xf020 + r.below(224), 0x1f600, 0x3b1, 0x2212, 0x2192, 0x263a, 0x10ffff, 0x110000, 0xd800];
        const cp = cpPool[r.below(r.below(3) === 0 ? cpPool.length : 4)];
        cps.push([cp === 0x110000 && r.below(4) !== 0 ? 0x41 : cp, jv(x), jv(base)]);
        // mostly increasing x on one baseline, now and then a new run
        const t = r.below(12);
        x = typeof x !== 'number' ? coord(r, 0, 400) : t === 0 ? x - 30 : t === 1 ? x + 1e-9 : x + 2 + unit(r) * 9;
        if (t === 2) base = typeof base !== 'number' ? 100 : base + 12;
      }
      const opts = {};
      if (r.below(3) === 0) opts.scaleX = [0.8, 1.2, 1, enc(NaN), 0][r.below(5)];
      if (r.below(4) === 0) opts.synthBold = true;
      if (r.below(4) === 0) opts.synthItalic = true;
      if (r.below(3) !== 0) opts.runs = true;
      ops.push(['glyphs', r.below(st.fonts), r.below(15) === 0 ? enc([0, -0, NaN, 0.0004][r.below(4)]) : 6 + unit(r) * 30, color(r), cps, opts]);
    } else if (k < 14) {
      const cmds = genPathCmds(r);
      const fill = r.below(3) === 0 ? null : color(r);
      const stroke = r.below(2) === 0 ? null : [color(r), pos(r, 6), r.below(2) === 0 ? [] : [pos(r, 5), pos(r, 5)]];
      ops.push(['path', cmds, fill, stroke, r.below(2) === 0]);
    } else if (k < 15 && st.shadings > 0) ops.push(['shaded', genPathCmds(r), r.below(st.shadings), r.below(2) === 0]);
    else if (k < 17) ops.push(['clipRect', c(r), c(r), pos(r), pos(r)], ['rect', c(r), c(r), pos(r), pos(r), color(r)], ['pop']);
    else if (k < 18) ops.push(['clipPath', genPathCmds(r), r.below(2) === 0], ['line', c(r), c(r), c(r), c(r), color(r), 1, []], ['pop']);
    else if (k < 20 && st.images > 0) ops.push(['imgMatrix', r.below(st.images), c(r, -2, 2), c(r, -2, 2), c(r, -2, 2), c(r, -2, 2), c(r), c(r)]);
    else if (k < 21) ops.push(['xform', c(r, -2, 2), c(r, -2, 2), c(r, -2, 2), c(r, -2, 2), c(r), c(r)], ['rect', 0, 0, 10, 10, color(r)], ['pop']);
    else if (k < 23 && st.images > 0) ops.push(['imgT', r.below(st.images), c(r), c(r), pos(r), pos(r), r.below(3) === 0 ? 0 : [90, 45, -30, 180, 360, 17.5, enc(NaN)][r.below(7)], r.below(2) === 0, r.below(2) === 0]);
    else if (k < 25 && st.images > 0) ops.push(['img', r.below(st.images), c(r), c(r), pos(r), pos(r)]);
    else if (k < 27) {
      const kind = r.below(6);
      const link = { x: c(r, 0, 500), y: c(r, 0, 700), w: pos(r, 200), h: pos(r, 30) };
      if (kind === 0) link.uri = 'https://example.com/a b(c)\\d"<>\u{e9}' + r.below(100);
      else if (kind === 1) link.uri = ['  MAILTO:a@b.c ', 'javascript:alert(1)', '', 'HTTP://X', 'ftp://x', ' \u{a0}https://sp.ce', '\u{feff}http://bom', 'httpss://x'][r.below(8)];
      else if (kind === 2) { link.pageIndex = r.below(6) - 2; link.destTop = r.below(2) === 0 ? c(r, 0, 800) : undefined; }
      else if (kind === 3) { link.pageIndex = r.below(3); }
      else if (kind === 4) { link.uri = 'http://x/' + r.below(10); link.pageIndex = 1; }
      ops.push(['link', link]);
    } else if (k < 29 && depth < 2) {
      ops.push(['alpha', r.below(4) === 0 ? null : [0, 0.5, 0.25, 1, 0.0004, 0.9996, enc(NaN), -0.5, 2][r.below(9)], r.below(3) === 0 ? null : [0, 0.5, 0.25, 1, 0.3333][r.below(5)], genPageOps(r, st, depth + 1)]);
    } else {
      const res = genResource(r, st);
      if (res) ops.push(res);
    }
  }
  return ops;
}

function genScript(seed) {
  const r = new Rng(seed);
  const st = { fonts: 0, images: 0, shadings: 0 };
  const ops = [];
  for (let i = r.below(4); i > 0; i--) { const res = genResource(r, st); if (res) ops.push(res); }
  const pages = 1 + r.below(3);
  for (let p = 0; p < pages; p++) {
    const w = r.below(8) === 0 ? coord(r, 50, 900) : [612, 595.276, 841.89, 792, 200.5][r.below(5)];
    const h = r.below(8) === 0 ? coord(r, 50, 1200) : [792, 841.89, 1190.55, 612, 300][r.below(5)];
    ops.push(['page', w, h, genPageOps(r, st, 0)]);
    if (r.below(3) === 0) { const res = genResource(r, st); if (res) ops.push(res); }
  }
  const outlines = r.below(4) === 0 ? 0 : r.below(9);
  for (let i = 0; i < outlines; i++) {
    const titles = ['Chapter ' + i, '  ', '', 'T\u{ed}tulo \u{e9} \u{1f600}', 'a(b)\\', 'x'.repeat(5), '\u{a0}'];
    ops.push(['outline', titles[r.below(titles.length)], r.below(6) === 0 ? [0, -1, 7][r.below(3)] : 1 + r.below(4), r.below(5) - 1, coord(r, 0, 800)]);
  }
  return ops;
}

// ---- interpreter over the TS API ----------------------------------------------------------

const color3 = (v) => v.map(dec);
const nums = (a) => a.map(dec);

function noise(seed, len) { const r = new Rng(seed); const b = new Uint8Array(len); for (let i = 0; i < len; i++) b[i] = r.next() & 255; return b; }

export function rgbaPixels(seed, w, h, mode) {
  const r = new Rng(seed);
  const extra = mode === 5 ? 2 : 0;
  const n = w * h + extra;
  const out = new Uint8Array(n * 4 + (mode === 5 ? 3 : 0));
  const flat = mode === 2 || mode === 3;
  const alpha = mode === 1 || mode === 3 || mode === 5;
  for (let i = 0; i < n; i++) {
    for (let j = 0; j < 3; j++) out[i * 4 + j] = flat ? r.below(4) * 60 : r.next() & 255;
    out[i * 4 + 3] = alpha && r.below(3) !== 0 ? (r.below(2) === 0 ? 255 : r.below(256)) : 255;
  }
  if (mode === 5) { out[n * 4] = 1; out[n * 4 + 1] = 2; out[n * 4 + 2] = 3; }
  return out;
}

function runScript(ops, compress) {
  const doc = new PdfDocument({ compress });
  const F = []; const I = []; const S = [];
  const res = (op) => {
    switch (op[0]) {
      case 'font': {
        const [, file, name, prog] = op;
        const sf = fonts[file];
        const m = sf.metrics;
        let program;
        if (prog[0] === 'face') program = { unitsPerEm: m.unitsPerEm, ascender: m.ascender, descender: m.descender, glyphId: (cp) => sf.glyphId(cp), advanceWidth: (g) => sf.advanceWidth(g) };
        else if (prog[0] === 'sym') {
          program = {
            unitsPerEm: m.unitsPerEm, ascender: m.ascender, descender: m.descender, advanceWidth: (g) => sf.advanceWidth(g),
            glyphId: (cp) => { const g = sf.glyphId(cp); return g === 0 && cp >= 0xf000 && cp <= 0xf0ff ? sf.glyphId(cp - 0xf000) : g; },
          };
        } else {
          const [, upem, asc, desc, bbox, italic] = prog;
          program = { unitsPerEm: dec(upem), glyphId: (cp) => sf.glyphId(cp), advanceWidth: (g) => sf.advanceWidth(g) };
          if (asc !== null) program.ascender = dec(asc);
          if (desc !== null) program.descender = dec(desc);
          if (bbox !== null) program.fontBBox = nums(bbox);
          if (italic !== null) program.italicAngle = dec(italic);
        }
        F.push(doc.embedFont(name, fontBytes[file], program));
        return true;
      }
      case 'jpeg': {
        const [, seed, len, w, h, comps, inv, decode, mask] = op;
        I.push(doc.embedJpeg(noise(seed, len), w, h, comps, inv === 1, decode === null ? undefined : nums(decode),
          mask === null ? undefined : { alpha: noise(mask[0], mask[1]), width: mask[2], height: mask[3] }));
        return true;
      }
      case 'rgba': I.push(doc.embedRgba(rgbaPixels(op[1], op[2], op[3], op[4]), op[2], op[3])); return true;
      case 'shading': {
        const [, kind, g, ns, seed, ph] = op;
        const gg = nums(g);
        const geom = kind === 0 ? { kind: 'axial', x0: gg[0], y0: gg[1], x1: gg[2], y1: gg[3] } : { kind: 'radial', x0: gg[0], y0: gg[1], r: gg[2] };
        S.push(doc.addShading(geom, noise(seed, ns), dec(ph)));
        return true;
      }
      default: return false;
    }
  };
  const cmds = (list) => list.map((cm) => (cm[0] === 'Z' ? { op: 'Z' } : cm[0] === 'C'
    ? { op: 'C', x1: dec(cm[1]), y1: dec(cm[2]), x2: dec(cm[3]), y2: dec(cm[4]), x: dec(cm[5]), y: dec(cm[6]) } : { op: cm[0], x: dec(cm[1]), y: dec(cm[2]) }));
  const page = (p, ops2) => {
    for (const op of ops2) {
      if (res(op)) continue;
      switch (op[0]) {
        case 'rect': p.fillRect(dec(op[1]), dec(op[2]), dec(op[3]), dec(op[4]), color3(op[5])); break;
        case 'line': p.line(dec(op[1]), dec(op[2]), dec(op[3]), dec(op[4]), color3(op[5]), dec(op[6]), nums(op[7])); break;
        case 'glyphs': {
          const font = F[op[1]];
          const placed = op[4].map(([cp, x, base]) => ({ gid: font.encode(cp).gid, codePoint: cp, xPt: dec(x), baselineFromTopPt: dec(base) }));
          const o = { ...op[5] };
          if (o.scaleX !== undefined) o.scaleX = dec(o.scaleX);
          p.glyphs(font, dec(op[2]), color3(op[3]), placed, o);
          break;
        }
        case 'path': p.path(cmds(op[1]), op[2] === null ? undefined : color3(op[2]), op[3] === null ? undefined : { color: color3(op[3][0]), widthPt: dec(op[3][1]), dash: nums(op[3][2]) }, op[4]); break;
        case 'shaded': p.fillShaded(cmds(op[1]), S[op[2]], op[3]); break;
        case 'clipRect': p.pushClipRect(dec(op[1]), dec(op[2]), dec(op[3]), dec(op[4])); break;
        case 'pop': p.popClip(); break;
        case 'clipPath': p.pushClipPath(cmds(op[1]), op[2]); break;
        case 'imgMatrix': p.drawImageMatrix(I[op[1]], ...op.slice(2).map(dec)); break;
        case 'xform': p.pushTransform(...op.slice(1).map(dec)); break;
        case 'imgT': p.drawImageTransformed(I[op[1]], dec(op[2]), dec(op[3]), dec(op[4]), dec(op[5]), dec(op[6]), op[7], op[8]); break;
        case 'img': p.drawImage(I[op[1]], dec(op[2]), dec(op[3]), dec(op[4]), dec(op[5])); break;
        case 'link': {
          const l = op[1];
          p.addLink({ xPt: dec(l.x), yTopPt: dec(l.y), wPt: dec(l.w), hPt: dec(l.h), uri: l.uri, pageIndex: l.pageIndex, destTopPt: l.destTop === undefined ? undefined : dec(l.destTop) });
          break;
        }
        case 'alpha': p.withAlpha(op[1] === null ? undefined : dec(op[1]), op[2] === null ? undefined : dec(op[2]), () => page(p, op[3])); break;
        default: throw new Error(`op ${op[0]}`);
      }
    }
  };
  for (const op of ops) {
    if (res(op)) continue;
    if (op[0] === 'page') page(doc.addPage(dec(op[1]), dec(op[2])), op[3]);
    else if (op[0] === 'outline') doc.addOutline({ title: op[1], level: op[2], pageIndex: op[3], topPt: dec(op[4]) });
    else throw new Error(`op ${op[0]}`);
  }
  return doc.build();
}

// ---- structural comparison ----------------------------------------------------------------

const latin1 = (u8) => Buffer.from(u8).toString('latin1');

/** Parses a classic-xref PDF of the writer; validates the xref; returns the objects. */
export function parsePdf(u8) {
  const text = latin1(u8);
  const sx = text.lastIndexOf('startxref\n');
  if (sx < 0) throw new Error('no startxref');
  const xrefAt = Number(/^\d+/.exec(text.slice(sx + 10))[0]);
  if (!text.startsWith('xref\n0 ', xrefAt)) throw new Error('bad xref');
  const m = /^xref\n0 (\d+)\n/.exec(text.slice(xrefAt, xrefAt + 40));
  const count = Number(m[1]);
  let p = xrefAt + m[0].length;
  if (text.slice(p, p + 20) !== '0000000000 65535 f \n') throw new Error('bad free entry');
  p += 20;
  const offs = [];
  for (let i = 1; i < count; i++, p += 20) {
    const e = text.slice(p, p + 20);
    if (!/^\d{10} 00000 n \n$/.test(e)) throw new Error(`bad xref entry ${i}`);
    offs.push(Number(e.slice(0, 10)));
  }
  const tr = /^trailer\n<< \/Size (\d+) \/Root (\d+) 0 R >>\nstartxref\n(\d+)\n%%EOF\n$/.exec(text.slice(p));
  if (!tr || Number(tr[1]) !== count || Number(tr[3]) !== xrefAt) throw new Error('bad trailer');
  if (!text.startsWith('%PDF-1.7\n%\u{e2}\u{e3}\u{cf}\u{d3}\n')) throw new Error('bad header');
  const objs = [];
  for (let i = 0; i < offs.length; i++) {
    const head = `${i + 1} 0 obj\n`;
    if (!text.startsWith(head, offs[i])) throw new Error(`xref entry ${i + 1} does not hit its object`);
    const end = (i + 1 < offs.length ? offs[i + 1] : xrefAt) - 8;
    if (text.slice(end, end + 8) !== '\nendobj\n') throw new Error(`object ${i + 1} not closed`);
    objs.push(u8.subarray(offs[i] + head.length, end));
  }
  return { objs, root: Number(tr[2]), count };
}

const STREAM = /^(<<[\s\S]*? \/Length (\d+) >>)\nstream\n/;

/** The normalized form of the file: per object the dictionary without /Filter /FlateDecode and with /Length as `N`, the payload inflated. */
export function normalize(u8) {
  const { objs, root, count } = parsePdf(u8);
  const s = new Stream();
  s.u32(count).u32(root);
  let deflated = 0;
  objs.forEach((body, i) => {
    s.tag('o').u32(i + 1);
    const t = latin1(body.subarray(0, Math.min(body.length, 4000)));
    const m = STREAM.exec(t);
    if (!m) { s.tag('d').digestOf(body); return; }
    const len = Number(m[2]);
    const headLen = m[0].length;
    const payload = body.subarray(headLen, headLen + len);
    if (latin1(body.subarray(headLen + len)) !== '\nendstream') throw new Error(`object ${i + 1}: bad endstream`);
    const flate = m[1].includes(' /Filter /FlateDecode');
    const dict = m[1].replace(' /Filter /FlateDecode', '').replace(/ \/Length \d+ >>$/, ' /Length N >>');
    let data = payload;
    if (flate) { data = inflateSync(payload); deflated++; }
    s.tag('s').str(dict).digestOf(data);
  });
  return { objects: count - 1, deflated, digest: s.finish().short };
}

// ---- facets of the scalar functions -------------------------------------------------------

function doubleFromBits(r) {
  const b = Buffer.alloc(8);
  b.writeUInt32LE(r.next(), 0);
  b.writeUInt32LE(r.next(), 4);
  return b.readDoubleLE(0);
}

export function fmtInputs() {
  const out = [0, -0, NaN, Infinity, -Infinity, 1e9, -1e9, 1e9 + 0.5, 1e10, 0.0005, 0.0015, -0.0005, 0.0004999999999999999, 0.0025, 1.0005, 2.5, -2.5, 0.1 + 0.2, 1 / 3, 123456.78912, 5e-324, 1e300, -1e300, 999999999.9996, 0.9995, 0.99949999];
  const r = new Rng(0x5eed);
  for (let i = 0; i < 20000; i++) out.push(doubleFromBits(r));
  for (let i = 0; i < 20000; i++) out.push((r.next() % 2000001 - 1000000) / 1000 + (r.below(3) === 0 ? 0.0005 : 0));
  for (let i = 0; i < 20000; i++) out.push(unit(r) * 2000 - 1000);
  for (let i = 0; i < 5000; i++) out.push((r.next() % 4000000000 - 2000000000) * (r.below(2) === 0 ? 1 : 0.001));
  return out;
}

const HEX_PIECES = ['#', '0', '9', 'a', 'F', 'g', 'x', ' ', '-', '+', '\t', '\u{e9}', '\u{1F600}', 'au', 'FF', '12', 'zz', '0x', '\u{a0}', '\u{feff}', '\u{2003}', 'E', 'e', '.', '#'];
export function hexInputs() {
  const out = ['', '#', 'auto', '#FF8800', 'ff8800', '#ff00', 'abcdef12', ' ff0000', '-1 1 1', '# f0f0f', '0x1020 30', '#12#34#56', '\u{e9}1\u{e9}2\u{e9}3', '\u{1F600}\u{1F600}\u{1F600}'];
  const r = new Rng(0xc010);
  for (let i = 0; i < 3000; i++) { let s = ''; for (let k = r.below(8); k > 0; k--) s += HEX_PIECES[r.below(HEX_PIECES.length)]; out.push(s); }
  return out;
}

const URI_SCHEMES = ['http://', 'https://', 'HTTPS://', 'mailto:', 'MailTo:', 'ftp://', 'javascript:'];
const URI_PIECES = ['http://', 'https://', 'HTTPS://', 'mailto:', 'MailTo:', 'ftp://', 'javascript:', ' ', '\t', '\u{a0}', '\u{feff}', '\u{2003}', 'a', 'Z', '0', '(', ')', '\\', '"', '<', '>', '%', '\u{e9}', '\u{20ac}', '\u{1F600}', '\u0085', '\u{180e}', '\u{200b}', '\n', '\u007f', '.', '/', '?', '#', '&', 'K', '\u{17f}'];
export function uriInputs() {
  const out = ['', ' ', 'http://', 'mailto:', 'http:/x', 'HtTp://a b', '  https://x  ', 'https://example.com/path?q=a b&c=(d)', '\u{17f}http://x', 'htKp://x'];
  const r = new Rng(0x0ff1);
  for (let i = 0; i < 4000; i++) {
    let s = r.below(3) === 0 ? '' : URI_SCHEMES[r.below(URI_SCHEMES.length)];
    for (let k = r.below(10); k > 0; k--) s += URI_PIECES[r.below(URI_PIECES.length)];
    out.push(s);
  }
  return out;
}

function main() {
  const cap = new CaptureWriter(['PP0 of the uo-pdf writer layer (frontier W): frozen TS pdf-writer driven by generated op scripts', 'regenerate: see tests/parity_support/capture_writer.mjs']);
  if (DUMP) mkdirSync(DUMP, { recursive: true });
  let deflatedTotal = 0; let throws = 0;
  for (let i = 0; i < SCRIPTS; i++) {
    const key = `s${String(i).padStart(4, '0')}`;
    const ops = genScript(0x1000 + i * 7919);
    cap.set(key, 'script', JSON.stringify(ops));
    let raw; let comp;
    try { raw = runScript(ops, false); comp = runScript(ops, true); } catch (e) {
      // the TS throws where String.fromCodePoint gets a code point past U+10FFFF (ToUnicode)
      if (!(e instanceof RangeError)) throw e;
      cap.set(key, 'raw', 'throw'); cap.set(key, 'norm', 'throw'); cap.set(key, 'deflated', 'throw');
      throws++;
      continue;
    }
    const norm = normalize(comp);
    const rawNorm = normalize(raw);
    if (norm.objects !== rawNorm.objects) throw new Error(`${key}: object count differs between modes`);
    if (norm.digest !== rawNorm.digest) throw new Error(`${key}: the TS compressed and uncompressed files are not structurally equal`);
    deflatedTotal += norm.deflated;
    cap.set(key, 'raw', `${raw.length}:${createHash('sha256').update(raw).digest('hex').slice(0, 32)}`);
    cap.set(key, 'norm', `${norm.objects}:${norm.digest}`);
    cap.set(key, 'deflated', norm.deflated);
    if (DUMP) { writeFileSync(join(DUMP, `${key}.raw.pdf`), raw); writeFileSync(join(DUMP, `${key}.comp.pdf`), comp); }
  }
  // scalar facets
  const sFmt = new Stream();
  for (const v of fmtInputs()) sFmt.str(fmt(v));
  cap.set('scalars', 'fmt', sFmt.finish().short);
  const sHex = new Stream();
  for (const v of hexInputs()) for (const x of hexColor(v)) sHex.f64(x);
  cap.set('scalars', 'hexColor', sHex.finish().short);
  const sUri = new Stream();
  for (const v of uriInputs()) { const u = safePdfUri(v); if (u === null) sUri.tag('n'); else sUri.tag('s').str(u); }
  cap.set('scalars', 'safePdfUri', sUri.finish().short);
  const sHexU = new Stream();
  for (const v of uriInputs()) sHexU.str(utf16beHex(v));
  cap.set('scalars', 'utf16beHex', sHexU.finish().short);
  cap.meta('scripts', SCRIPTS);
  cap.meta('deflated-streams-ts', deflatedTotal);
  cap.meta('scripts-where-the-ts-throws', throws);
  cap.write(join(here, '../parity/writer.cap'));
  process.stdout.write(`wrote ${SCRIPTS} scripts, ${deflatedTotal} deflated streams in the TS compressed files\n`);
}

if (import.meta.url === `file://${process.argv[1].replace(/\\/g, '/')}` || process.argv[1]?.endsWith('capture_writer.mjs')) main();
