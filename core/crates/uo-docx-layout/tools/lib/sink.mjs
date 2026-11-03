// A canonical stream that can also keep its raw bytes: the testkit `Stream` (SHA-256 of the item
// sequence, src/stream.rs encoding) plus, with `raw: true`, every byte written, so the Rust side
// can decode the TS layout from the same sequence (the adapter check of the F0 harness: TS layout
// -> stream bytes -> Rust `Layout` -> Rust Canon -> same digest).
//
// Encodings added on top of the testkit items (both sides, see src/canon.rs):
//   num(v)     f64 with NaN canonical (JS keeps no NaN payload)
//   optNum(v)  bool(v !== undefined && v !== null) [+ num(v)]
//   u16s(s)    the UTF-16 code units of a JS string as `bytes` (2 bytes each, little-endian):
//              lone surrogates kept (the testkit `str` is UTF-8 and would turn them into U+FFFD)
//   optU16s(s) bool(s !== undefined && s !== null) [+ u16s(s)]
import { createHash } from 'node:crypto';
import { Stream } from '../../../uo-testkit/js/testkit.mjs';

const CHUNK = 1 << 16;

export class Sink {
  constructor({ raw = false, trace = false } = {}) {
    this.s = new Stream({ trace });
    this.raw = raw ? [] : null;
    this.cur = raw ? Buffer.allocUnsafe(CHUNK) : null;
    this.at = 0;
  }
  #push(u8) {
    if (!this.raw) return;
    if (this.at + u8.length > CHUNK) { this.raw.push(Buffer.from(this.cur.subarray(0, this.at))); this.at = 0; }
    if (u8.length > CHUNK) { this.raw.push(Buffer.from(u8)); return; }
    this.cur.set(u8, this.at);
    this.at += u8.length;
  }
  tag(c) { this.s.tag(c); if (this.raw) this.#push(Uint8Array.of(c.charCodeAt(0))); return this; }
  u32(v) { this.s.u32(v); if (this.raw) { const b = Buffer.alloc(4); b.writeUInt32LE(v >>> 0); this.#push(b); } return this; }
  num(v) {
    const x = v !== v ? NaN : v;
    this.s.f64(x);
    if (this.raw) { const b = Buffer.alloc(8); b.writeDoubleLE(x); this.#push(b); }
    return this;
  }
  bool(v) { this.s.bool(v); if (this.raw) this.#push(Uint8Array.of(v ? 1 : 0)); return this; }
  bytes(u8) {
    this.s.bytes(u8);
    if (this.raw) { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(u8.length)); this.#push(b); this.#push(u8); }
    return this;
  }
  digestOf(u8) {
    this.s.digestOf(u8);
    if (this.raw) {
      const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(u8.length)); this.#push(b);
      this.#push(createHash('sha256').update(u8).digest());
    }
    return this;
  }
  optNum(v) { const has = v !== undefined && v !== null; this.bool(has); if (has) this.num(v); return this; }
  u16s(str) {
    const s = String(str);
    const u8 = new Uint8Array(s.length * 2);
    for (let i = 0; i < s.length; i++) { const c = s.charCodeAt(i); u8[2 * i] = c & 0xff; u8[2 * i + 1] = c >>> 8; }
    return this.bytes(u8);
  }
  optU16s(s) { const has = s !== undefined && s !== null; this.bool(has); if (has) this.u16s(s); return this; }
  get items() { return this.s.items; }
  /** `count:digest` (the form kept in capture files) and, with raw, the bytes. */
  finish() {
    const n = this.s.items;
    const d = this.s.finish();
    let bytes = null;
    if (this.raw) { this.raw.push(Buffer.from(this.cur.subarray(0, this.at))); bytes = Buffer.concat(this.raw); }
    return { value: `${n}:${d.short}`, bytes, trace: this.s.trace() };
  }
}
