// Small XML-text builders and the picture bytes of the chart generator (gen/charts.mjs).
import { crc32 } from 'node:zlib';
import { deflateSync } from 'node:zlib';

const esc = (v) => String(v).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
export const escText = (v) => String(v).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

export const attrs = (o) => Object.entries(o ?? {}).filter(([, v]) => v !== undefined && v !== null).map(([k, v]) => ` ${k}="${esc(v)}"`).join('');

/** `<name a="b">children</name>` (self-closing without children; arrays flattened; null/false/undefined skipped). */
export const el = (name, a, ...kids) => {
  const body = kids.flat(Infinity).filter((x) => x != null && x !== false).join('');
  return body ? `<${name}${attrs(a)}>${body}</${name}>` : `<${name}${attrs(a)}/>`;
};
export const val = (name, v) => el(name, { val: v });
export const text = (name, a, t) => `<${name}${attrs(a)}>${escText(t)}</${name}>`;

export const NS = {
  c: 'http://schemas.openxmlformats.org/drawingml/2006/chart',
  a: 'http://schemas.openxmlformats.org/drawingml/2006/main',
  r: 'http://schemas.openxmlformats.org/officeDocument/2006/relationships',
  c14: 'http://schemas.microsoft.com/office/drawing/2007/8/2/chart',
  c15: 'http://schemas.microsoft.com/office/drawing/2012/chart',
  mc: 'http://schemas.openxmlformats.org/markup-compatibility/2006',
  cdr: 'http://schemas.openxmlformats.org/drawingml/2006/chartDrawing',
  cx: 'http://schemas.microsoft.com/office/drawing/2014/chartex',
};
export const nsDecl = (...ps) => ps.map((p) => ` xmlns:${p}="${NS[p]}"`).join('');

// ------------------------------------------------------------------------------------------------ picture bytes
const be32 = (n) => { const b = Buffer.alloc(4); b.writeUInt32BE(n >>> 0); return b; };
const pngChunk = (type, data) => {
  const t = Buffer.from(type, 'latin1');
  return Buffer.concat([be32(data.length), t, data, be32(crc32(Buffer.concat([t, data])))]);
};

/** A valid small PNG (RGB, w x h, a diagonal ramp) with an optional `pHYs` chunk (`dpi`: pixels per inch). */
export function makePng(w, h, dpi) {
  const ihdr = Buffer.concat([be32(w), be32(h), Buffer.from([8, 2, 0, 0, 0])]);
  const rows = [];
  for (let y = 0; y < h; y++) {
    const row = Buffer.alloc(1 + w * 3);
    for (let x = 0; x < w; x++) { row[1 + x * 3] = (x * 255 / Math.max(1, w - 1)) | 0; row[2 + x * 3] = (y * 255 / Math.max(1, h - 1)) | 0; row[3 + x * 3] = 128; }
    rows.push(row);
  }
  const parts = [Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), pngChunk('IHDR', ihdr)];
  if (dpi) { const ppm = Math.round(dpi / 0.0254); parts.push(pngChunk('pHYs', Buffer.concat([be32(ppm), be32(ppm), Buffer.from([1])]))); }
  parts.push(pngChunk('IDAT', deflateSync(Buffer.concat(rows))), pngChunk('IEND', Buffer.alloc(0)));
  return new Uint8Array(Buffer.concat(parts));
}

/** A JPEG skeleton (SOI, JFIF APP0 with `units`/density, optional EXIF resolution, SOF0 of w x h, EOI): enough for the readers. */
export function makeJpeg(w, h, { units = 1, dens = 96, exif } = {}) {
  const u16 = (n) => Buffer.from([(n >> 8) & 255, n & 255]);
  const jfif = Buffer.concat([Buffer.from([0xff, 0xe0, 0, 16]), Buffer.from('JFIF\0', 'latin1'), Buffer.from([1, 1, units]), u16(dens), u16(dens), Buffer.from([0, 0])]);
  let ex = Buffer.alloc(0);
  if (exif) {
    // TIFF big-endian header + one IFD with XResolution, YResolution (rationals) and ResolutionUnit (2 = inch)
    const t = Buffer.alloc(8 + 2 + 3 * 12 + 4 + 16);
    t.write('MM', 0, 'latin1'); t.writeUInt16BE(42, 2); t.writeUInt32BE(8, 4); t.writeUInt16BE(3, 8);
    const ent = (i, tag, type, cnt, v) => { const o = 10 + i * 12; t.writeUInt16BE(tag, o); t.writeUInt16BE(type, o + 2); t.writeUInt32BE(cnt, o + 4); if (type === 5) t.writeUInt32BE(v, o + 8); else t.writeUInt16BE(v, o + 8); };
    const rat = 10 + 36 + 4;
    ent(0, 0x011a, 5, 1, rat); ent(1, 0x011b, 5, 1, rat + 8); ent(2, 0x0128, 3, 1, 2);
    t.writeUInt32BE(0, 10 + 36);
    t.writeUInt32BE(exif, rat); t.writeUInt32BE(1, rat + 4); t.writeUInt32BE(exif, rat + 8); t.writeUInt32BE(1, rat + 12);
    const body = Buffer.concat([Buffer.from('Exif\0\0', 'latin1'), t]);
    ex = Buffer.concat([Buffer.from([0xff, 0xe1]), u16(body.length + 2), body]);
  }
  const sof = Buffer.concat([Buffer.from([0xff, 0xc0, 0, 11, 8]), u16(h), u16(w), Buffer.from([1, 1, 0x11, 0])]);
  return new Uint8Array(Buffer.concat([Buffer.from([0xff, 0xd8]), jfif, ex, sof, Buffer.from([0xff, 0xd9])]));
}
