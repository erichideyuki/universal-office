// Tiny deterministic PNGs generated in code (RGBA 8-bit, no interlace). `pixel(x, y)` returns
// [r, g, b, a]. Pure Node (`node:zlib`); same arguments -> same bytes.
import { crc32, deflateSync } from 'node:zlib';

function chunk(type, data) {
  const t = Buffer.from(type, 'ascii');
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([t, data])) >>> 0, 0);
  return Buffer.concat([len, t, data, crc]);
}

export function makePng(w, h, pixel) {
  const raw = Buffer.alloc((w * 4 + 1) * h);
  let p = 0;
  for (let y = 0; y < h; y++) {
    raw[p++] = 0; // filter: none
    for (let x = 0; x < w; x++) {
      const [r, g, b, a] = pixel(x, y);
      raw[p++] = r; raw[p++] = g; raw[p++] = b; raw[p++] = a;
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8; ihdr[9] = 6; // 8 bit, RGBA
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

/** A seeded tiny picture: gradient, checker, rings or stripes (`kind` 0-3), sizes 4..24 px. */
export function randomPng(rng) {
  const w = 4 + rng.below(21);
  const h = 4 + rng.below(21);
  const c1 = [rng.below(256), rng.below(256), rng.below(256)];
  const c2 = [rng.below(256), rng.below(256), rng.below(256)];
  const alpha = rng.below(4) === 0 ? 128 + rng.below(128) : 255;
  const kind = rng.below(4);
  const mix = (t) => c1.map((c, i) => Math.round(c + (c2[i] - c) * t));
  return makePng(w, h, (x, y) => {
    let t;
    if (kind === 0) t = (x + y) / (w + h - 2);
    else if (kind === 1) t = (x >> 1) + (y >> 1) & 1;
    else if (kind === 2) t = Math.min(1, Math.sqrt((x - w / 2) ** 2 + (y - h / 2) ** 2) / (Math.max(w, h) / 2)); // sqrt: correctly rounded on every platform
    else t = x % 4 < 2 ? 0 : 1;
    return [...mix(t), alpha];
  });
}
