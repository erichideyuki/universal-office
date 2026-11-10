// The subset of Node's `Buffer` the testkit `Stream` and the recording context use: allocUnsafe, from(string | bytes | ArrayBuffer,
// offset, length), the little-endian writers/reader (they return offset + size like Node) and toString('hex' | 'utf8').
export class Buffer extends Uint8Array {
  static allocUnsafe(n) { return new Buffer(n); }
  static from(v, a, b) {
    if (typeof v === 'string') {
      if (a === 'base64') {
        const s = atob(v);
        const o = new Buffer(s.length);
        for (let i = 0; i < s.length; i++) o[i] = s.charCodeAt(i);
        return o;
      }
      const u8 = new TextEncoder().encode(v);
      const o = new Buffer(u8.length);
      o.set(u8);
      return o;
    }
    if (v instanceof ArrayBuffer) return new Buffer(v, a ?? 0, b ?? v.byteLength - (a ?? 0));
    const o = new Buffer(v.length ?? v.byteLength);
    o.set(v);
    return o;
  }
  #dv() { return new DataView(this.buffer, this.byteOffset, this.byteLength); }
  writeUInt32LE(v, o = 0) { this.#dv().setUint32(o, v, true); return o + 4; }
  writeBigUInt64LE(v, o = 0) { this.#dv().setBigUint64(o, v, true); return o + 8; }
  writeBigInt64LE(v, o = 0) { this.#dv().setBigInt64(o, v, true); return o + 8; }
  writeDoubleLE(v, o = 0) { this.#dv().setFloat64(o, v, true); return o + 8; }
  readBigUInt64LE(o = 0) { return this.#dv().getBigUint64(o, true); }
  toString(encoding = 'utf8') {
    if (encoding === 'hex') {
      let s = '';
      for (let i = 0; i < this.length; i++) s += this[i].toString(16).padStart(2, '0');
      return s;
    }
    return new TextDecoder().decode(this);
  }
}
