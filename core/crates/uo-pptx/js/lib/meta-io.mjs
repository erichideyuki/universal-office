// The container files of the B5b harness: the input set (`u32 keyLen, key, u32 n, bytes`, repeated) and the Rust recipe output
// (`u32 keyLen, key, u8 status, ...`), little endian.
export function packInputs(files) {
  const parts = [];
  for (const [key, bytes] of files) {
    const k = Buffer.from(key, 'utf8');
    const h = Buffer.alloc(4);
    h.writeUInt32LE(k.length);
    const n = Buffer.alloc(4);
    n.writeUInt32LE(bytes.length);
    parts.push(h, k, n, Buffer.from(bytes));
  }
  return Buffer.concat(parts);
}

export function unpackInputs(buf) {
  const out = [];
  let p = 0;
  while (p < buf.length) {
    const kl = buf.readUInt32LE(p); p += 4;
    const key = buf.toString('utf8', p, p + kl); p += kl;
    const n = buf.readUInt32LE(p); p += 4;
    out.push([key, new Uint8Array(buf.subarray(p, p + n))]); p += n;
  }
  return out;
}

/**
 * The output of `examples/meta_recipe.rs`: per (key, display) `u32 keyLen, key, u8 hasDisplay, f64 dw, f64 dh, u8 status` then for status 1 (recipe)
 * `u32 n, bytes`, 2 (bitmap) `u32 w, u32 h, u32 n, rgba`, 3 (needs) `u32 count`, 0 (not special).
 */
export function unpackOutputs(buf) {
  const out = [];
  let p = 0;
  while (p < buf.length) {
    const kl = buf.readUInt32LE(p); p += 4;
    const key = buf.toString('utf8', p, p + kl); p += kl;
    const hasD = buf[p++];
    const dw = buf.readDoubleLE(p); p += 8;
    const dh = buf.readDoubleLE(p); p += 8;
    const status = buf[p++];
    const e = { key, display: hasD ? { w: dw, h: dh } : null, status };
    if (status === 1) { const n = buf.readUInt32LE(p); p += 4; e.recipe = new Uint8Array(buf.subarray(p, p + n)); p += n; }
    else if (status === 2) { e.w = buf.readUInt32LE(p); e.h = buf.readUInt32LE(p + 4); const n = buf.readUInt32LE(p + 8); p += 12; e.rgba = new Uint8Array(buf.subarray(p, p + n)); p += n; }
    else if (status === 3) { e.needs = buf.readUInt32LE(p); p += 4; }
    else if (status === 4) { const n = buf.readUInt32LE(p); p += 4; e.bytes = new Uint8Array(buf.subarray(p, p + n)); p += n; }
    out.push(e);
  }
  return out;
}
