// The Node side of uo-testkit (core/crates/uo-testkit): the canonical stream encoding (the same
// bytes as src/stream.rs, hashed with SHA-256), the capture file writer (format in
// src/capture.rs) and helpers to load the frozen TS engine from a built checkout of main.
//
// The checkout comes from UO_TS_ROOT or --ts-root <dir>, never from a path written in the repo.
import { createHash } from 'node:crypto';
import { readdirSync, statSync, writeFileSync } from 'node:fs';
import { join, resolve, relative, sep } from 'node:path';
import { pathToFileURL } from 'node:url';

const CHUNK = 1 << 16;

/** Lone surrogates → U+FFFD, the UTF-8 encoding of a JS string (TextEncoder, Buffer). */
export const fffd = (s) => s.replace(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g, '�');

const hex = (u8) => Buffer.from(u8.buffer, u8.byteOffset, u8.byteLength).toString('hex');

/** A hashed canonical stream (see src/stream.rs for the encoding). */
export class Stream {
  constructor({ trace = false } = {}) {
    this.hash = createHash('sha256');
    this.buf = Buffer.allocUnsafe(CHUNK);
    this.at = 0;
    this.items = 0;
    this.lines = trace ? [] : null;
  }
  #room(n) {
    if (this.at + n > CHUNK) { this.hash.update(this.buf.subarray(0, this.at)); this.at = 0; }
  }
  #raw(u8) {
    if (u8.length > CHUNK) { this.hash.update(this.buf.subarray(0, this.at)); this.at = 0; this.hash.update(u8); return; }
    this.#room(u8.length);
    this.buf.set(u8, this.at);
    this.at += u8.length;
  }
  #note(line) { this.items++; if (this.lines) this.lines.push(line); }
  tag(c) { this.#room(1); this.buf[this.at++] = c.charCodeAt(0); this.#note(`tag ${c}`); return this; }
  u32(v) { this.#room(4); this.buf.writeUInt32LE(v >>> 0, this.at); this.at += 4; this.#note(`u32 ${v >>> 0}`); return this; }
  u64(v) { const b = BigInt(v); this.#room(8); this.buf.writeBigUInt64LE(b, this.at); this.at += 8; this.#note(`u64 ${b}`); return this; }
  i64(v) { const b = BigInt(v); this.#room(8); this.buf.writeBigInt64LE(b, this.at); this.at += 8; this.#note(`i64 ${b}`); return this; }
  f64(v) {
    this.#room(8); this.buf.writeDoubleLE(v, this.at);
    const bits = this.buf.readBigUInt64LE(this.at).toString(16).padStart(16, '0');
    this.at += 8; this.#note(`f64 ${bits}`); return this;
  }
  bool(v) { this.#room(1); this.buf[this.at++] = v ? 1 : 0; this.#note(`bool ${v ? 1 : 0}`); return this; }
  str(s) {
    const u8 = Buffer.from(s, 'utf8');
    this.#room(4); this.buf.writeUInt32LE(u8.length, this.at); this.at += 4;
    this.#raw(u8);
    this.#note(this.lines ? `str ${JSON.stringify(fffd(s))}` : '');
    return this;
  }
  bytes(u8) {
    this.#room(8); this.buf.writeBigUInt64LE(BigInt(u8.length), this.at); this.at += 8;
    this.#raw(u8);
    this.#note(this.lines ? `bytes ${u8.length} ${hex(u8.subarray(0, 32))}` : '');
    return this;
  }
  /** Length and SHA-256 of a payload (part bytes and the like). */
  digestOf(u8) {
    const d = createHash('sha256').update(u8).digest();
    this.#room(8); this.buf.writeBigUInt64LE(BigInt(u8.length), this.at); this.at += 8;
    this.#raw(d);
    this.#note(this.lines ? `digest ${u8.length} ${d.subarray(0, 16).toString('hex')}` : '');
    return this;
  }
  /** { hex, short } of everything written; the stream must not be used afterwards. */
  finish() {
    this.hash.update(this.buf.subarray(0, this.at));
    const h = this.hash.digest('hex');
    return { hex: h, short: h.slice(0, 32) };
  }
  trace() { return this.lines ? this.lines.join('\n') + '\n' : ''; }
}

/** Writer of a capture file (format in src/capture.rs): records in insertion order. */
export class CaptureWriter {
  constructor(header = []) { this.header = header; this.metas = []; this.records = new Map(); }
  meta(name, value) { this.metas.push([name, String(value)]); }
  set(key, facet, value) {
    if (/[\t\n\r]/.test(key) || /[\t\n\r=]/.test(facet) || /[\t\n\r]/.test(String(value))) throw new Error(`bad capture field ${key} ${facet}`);
    let r = this.records.get(key);
    if (!r) { r = []; this.records.set(key, r); }
    r.push([facet, String(value)]);
  }
  text() {
    const lines = this.header.map((h) => `# ${h}`);
    for (const [k, v] of this.metas) lines.push(`@${k} ${v}`);
    for (const [key, facets] of this.records) lines.push([key, ...facets.map(([f, v]) => `${f}=${v}`)].join('\t'));
    return lines.join('\n') + '\n';
  }
  write(path) { writeFileSync(path, this.text()); }
}

/** The built checkout of main: --ts-root <dir> or UO_TS_ROOT. */
export function tsRoot(argv = process.argv.slice(2)) {
  const i = argv.indexOf('--ts-root');
  const root = i >= 0 ? argv[i + 1] : process.env.UO_TS_ROOT;
  if (!root) throw new Error('set UO_TS_ROOT (or pass --ts-root) to a built checkout of main (dist/ and node_modules)');
  return resolve(root);
}

/** import() of a module of the checkout, by path relative to its root. */
export const importFrom = (root, rel) => import(pathToFileURL(join(root, rel)).href);

/** Files under `dir` (recursive, sorted by path) for which `pred(path)` holds. */
export function walk(dir, pred = () => true) {
  const out = [];
  const go = (d) => {
    for (const name of readdirSync(d).sort()) {
      const p = join(d, name);
      if (statSync(p).isDirectory()) go(p);
      else if (pred(p)) out.push(p);
    }
  };
  go(dir);
  return out.sort();
}

/** A path relative to `base` with `/` separators (keys of captures). */
export const relPath = (base, p) => relative(base, p).split(sep).join('/');

/**
 * Maps UTF-16 indices of `s` to UTF-8 byte offsets (positions captured from the TS are UTF-16
 * indices; Rust offsets are bytes of the same text). Lone surrogates count 3 bytes (U+FFFD).
 */
export function utf8Offsets(s) {
  // eslint-disable-next-line no-control-regex
  if (/^[\x00-\x7f]*$/.test(s)) return (i) => i;
  const table = new Float64Array(s.length + 1);
  let b = 0;
  for (let i = 0; i < s.length; i++) {
    table[i] = b;
    const c = s.charCodeAt(i);
    if (c < 0x80) b += 1;
    else if (c < 0x800) b += 2;
    else if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length && (s.charCodeAt(i + 1) & 0xfc00) === 0xdc00) { table[i + 1] = b; b += 4; i++; }
    else b += 3;
  }
  table[s.length] = b;
  return (i) => table[i];
}

/** xorshift32, the PRNG of src/gen.rs (`Rng`). */
export class Rng {
  constructor(seed) { this.s = (seed >>> 0) || 0x9e3779b9; }
  next() { let s = this.s; s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; this.s = s; return s; }
  below(n) { const v = this.next(); return n === 0 ? 0 : v % n; }
}
