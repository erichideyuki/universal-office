// Runtime of the schema-driven codec (G15) for TypeScript: the wire primitives the code emitted by
// core/tools/uo-schema is written against. The wire format is described in
// core/crates/uo-codec/src/lib.rs and is implemented identically there: little-endian, LEB128
// varints in the shortest form, raw f64 with a canonical NaN, UTF-16 strings with lone surrogates
// kept, struct bitmaps, bit-packed bool lists and columns. The decoder is strict (every
// non-canonical encoding is an error), so a message that decodes re-encodes to the same bytes.
//
// Erasable TypeScript only (no enums, no parameter properties): Node runs it with type stripping.

export interface Limits {
  /** Nesting of recursive types. */
  maxDepth: number;
  /** Elements of one list, columns value or table, and code units of one string. */
  maxListLen: number;
  /** Elements and code units of the whole message. */
  maxTotal: number;
}

export const DEFAULT_LIMITS: Limits = { maxDepth: 64, maxListLen: 1 << 24, maxTotal: 1 << 26 };

/** Most tables a schema may declare (as in the Rust runtime). */
export const MAX_TABLES = 16;

/** Why a message did not decode; `code` is the variant name of the Rust `DecodeError`. */
export class DecodeError extends Error {
  code: string;
  constructor(code: string, detail?: string) {
    super(detail === undefined ? code : `${code}: ${detail}`);
    this.name = "DecodeError";
    this.code = code;
  }
}

/** A value that cannot be encoded (a caller bug). */
export class EncodeError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "EncodeError";
  }
}

/** The symbol table: strings by id (UTF-16, as JavaScript holds them) and the reverse lookup. */
export class SymTable {
  list: string[] = [];
  private map = new Map<string, number>();

  get length(): number {
    return this.list.length;
  }

  /** Appends an entry; for a repeated string the first id stays the one `intern` answers. */
  push(s: string): void {
    this.list.push(s);
    if (!this.map.has(s)) this.map.set(s, this.list.length - 1);
  }

  /** The id of `s`, appended when it is new. */
  intern(s: string): number {
    let id = this.map.get(s);
    if (id === undefined) {
      id = this.list.length;
      this.push(s);
    }
    return id;
  }

  /** Drops the entries from `n` on. */
  truncate(n: number): void {
    for (let i = this.list.length - 1; i >= n; i--) {
      const s = this.list[i]!;
      if (this.map.get(s) === i) this.map.delete(s);
    }
    this.list.length = n;
  }
}

export function bitAt(bits: Uint8Array, i: number): boolean {
  const b = bits[i >> 3];
  return b !== undefined && ((b >> (i & 7)) & 1) === 1;
}

export function tagIndex(ty: string, tags: Map<string, number>, v: string): number {
  const i = tags.get(v);
  if (i === undefined) throw new EncodeError(`"${String(v)}" is not a variant of ${ty}`);
  return i;
}

export interface Header {
  message: number;
  generation: number;
}

/** Reads the wire format from untrusted bytes; every failure is a `DecodeError`. */
export class Reader {
  private buf: Uint8Array;
  private view: DataView;
  private pos = 0;
  private limits: Limits;
  private depth = 0;
  private total = 0;
  private bounds: number[] = new Array<number>(MAX_TABLES).fill(0);

  constructor(buf: Uint8Array, limits: Limits = DEFAULT_LIMITS) {
    this.buf = buf;
    this.view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    this.limits = limits;
  }

  remaining(): number {
    return this.buf.length - this.pos;
  }

  /** The message must end here. */
  end(): void {
    if (this.pos !== this.buf.length) throw new DecodeError("TrailingBytes");
  }

  private take(n: number): number {
    if (n > this.buf.length - this.pos) throw new DecodeError("Truncated");
    const at = this.pos;
    this.pos += n;
    return at;
  }

  u8(): number {
    if (this.pos >= this.buf.length) throw new DecodeError("Truncated");
    return this.buf[this.pos++]!;
  }

  bool(): boolean {
    const b = this.u8();
    if (b > 1) throw new DecodeError("BadBool");
    return b === 1;
  }

  /** LEB128 `u32`: at most 5 bytes, nothing over 2^32 - 1, shortest form only. */
  u32(): number {
    let value = 0;
    for (let i = 0; i < 5; i++) {
      const b = this.u8();
      if (i === 4 && (b & 0xf0) !== 0) throw new DecodeError("BadVarint");
      value += (b & 0x7f) * 2 ** (7 * i);
      if ((b & 0x80) === 0) {
        if (i > 0 && b === 0) throw new DecodeError("BadVarint");
        return value;
      }
    }
    throw new DecodeError("BadVarint");
  }

  u16(): number {
    const v = this.u32();
    if (v > 0xffff) throw new DecodeError("BadVarint");
    return v;
  }

  i32(): number {
    const z = this.u32();
    return (z >>> 1) ^ -(z & 1);
  }

  /** 8 raw bytes; a NaN other than the canonical one is refused. */
  f64(): number {
    const at = this.take(8);
    const v = this.view.getFloat64(at, true);
    if (v !== v && (this.view.getUint32(at, true) !== 0 || this.view.getUint32(at + 4, true) !== 0x7ff80000)) {
      throw new DecodeError("NonCanonicalNaN");
    }
    return v;
  }

  private charge(n: number): void {
    this.total += n;
    if (this.total > this.limits.maxTotal) throw new DecodeError("Limit", "total elements");
  }

  /** The count of a list or table whose elements take at least `minBits` bits each. */
  listLen(minBits: number): number {
    const n = this.u32();
    if (n > this.limits.maxListLen) throw new DecodeError("Limit", "list length");
    if (n * minBits > this.remaining() * 8) throw new DecodeError("Truncated");
    this.charge(n);
    return n;
  }

  /** A list: the count (checked against `minBits` per element), then `read` for each element. */
  list<T>(minBits: number, read: () => T): T[] {
    const n = this.listLen(minBits);
    const out: T[] = [];
    for (let i = 0; i < n; i++) out.push(read());
    return out;
  }

  /** A string of UTF-16 code units (lone surrogates kept). */
  utf16(): string {
    const n = this.listLen(16);
    const at = this.take(n * 2);
    let out = "";
    const chunk: number[] = [];
    for (let i = 0; i < n; i++) {
      chunk.push(this.view.getUint16(at + i * 2, true));
      if (chunk.length === 4096) {
        out += String.fromCharCode.apply(null, chunk);
        chunk.length = 0;
      }
    }
    if (chunk.length > 0) out += String.fromCharCode.apply(null, chunk);
    return out;
  }

  /** An inline byte string (a copy). */
  bytes(): Uint8Array {
    const n = this.listLen(8);
    const at = this.take(n);
    return this.buf.slice(at, at + n);
  }

  /** `n` bit-packed booleans: ceil(n/8) bytes whose padding bits are zero (read with `bitAt`). */
  bits(n: number): Uint8Array {
    const bytes = Math.ceil(n / 8);
    const at = this.take(bytes);
    const s = this.buf.subarray(at, at + bytes);
    if (n % 8 !== 0 && (s[bytes - 1]! >> (n % 8)) !== 0) throw new DecodeError("NonZeroPadding");
    return s;
  }

  /** The bitmap of a struct with `n` bit fields (read with `bitAt`). */
  bitmap(n: number): Uint8Array {
    return this.bits(n);
  }

  bitList(): boolean[] {
    const n = this.listLen(1);
    const bits = this.bits(n);
    const out: boolean[] = [];
    for (let i = 0; i < n; i++) out.push(bitAt(bits, i));
    return out;
  }

  enter(): void {
    if (++this.depth > this.limits.maxDepth) throw new DecodeError("Limit", "depth");
  }

  leave(): void {
    this.depth--;
  }

  /** An enum tag below `count`. */
  tag(ty: string, count: number): number {
    const t = this.u32();
    if (t >= count) throw new DecodeError("BadTag", `${t} is not a variant of ${ty}`);
    return t;
  }

  /** Checks magic and schema hash; answers the message id and the generation. */
  header(magic: Uint8Array, hash: Uint8Array): Header {
    const m = this.take(4);
    for (let i = 0; i < 4; i++) if (this.buf[m + i] !== magic[i]) throw new DecodeError("BadMagic");
    const h = this.take(8);
    for (let i = 0; i < 8; i++) if (this.buf[h + i] !== hash[i]) throw new DecodeError("VersionMismatch");
    return { message: this.u32(), generation: this.u32() };
  }

  /** `from` and `count` of one table's entries; `from` must be the table's length. Sets the bound of ids into it. */
  tableHeader(table: number, name: string, len: number, minBits: number): number {
    const from = this.u32();
    if (from !== len) throw new DecodeError("TableGap", `${name}: entries start at ${from}, expected ${len}`);
    const count = this.listLen(minBits);
    if (len + count > 0xffffffff) throw new DecodeError("Limit", "table length");
    this.bounds[table] = len + count;
    return count;
  }

  /** An id into table `table`, below the bound set by `tableHeader`. */
  ref(table: number, name: string): number {
    const i = this.u32();
    const bound = this.bounds[table] ?? 0;
    if (i >= bound) throw new DecodeError("BadRef", `${name} id ${i} out of range (bound ${bound})`);
    return i;
  }
}

/** Builds a message in a growable buffer. */
export class Writer {
  private buf: Uint8Array;
  private view: DataView;
  private pos = 0;

  constructor(capacity = 256) {
    this.buf = new Uint8Array(capacity);
    this.view = new DataView(this.buf.buffer);
  }

  private reserve(n: number): void {
    const need = this.pos + n;
    if (need <= this.buf.length) return;
    let cap = this.buf.length * 2;
    while (cap < need) cap *= 2;
    const next = new Uint8Array(cap);
    next.set(this.buf.subarray(0, this.pos));
    this.buf = next;
    this.view = new DataView(next.buffer);
  }

  u8(v: number): void {
    if (!Number.isInteger(v) || v < 0 || v > 0xff) throw new EncodeError(`${String(v)} is not a u8`);
    this.reserve(1);
    this.buf[this.pos++] = v;
  }

  bool(v: boolean): void {
    this.reserve(1);
    this.buf[this.pos++] = v ? 1 : 0;
  }

  /** LEB128, shortest form. */
  u32(v: number): void {
    if (!Number.isInteger(v) || v < 0 || v > 0xffffffff) throw new EncodeError(`${String(v)} is not a u32`);
    this.reserve(5);
    while (v > 0x7f) {
      this.buf[this.pos++] = (v & 0x7f) | 0x80;
      v >>>= 7;
    }
    this.buf[this.pos++] = v;
  }

  u16(v: number): void {
    if (!Number.isInteger(v) || v < 0 || v > 0xffff) throw new EncodeError(`${String(v)} is not a u16`);
    this.u32(v);
  }

  /** Zigzag, then LEB128. */
  i32(v: number): void {
    if (!Number.isInteger(v) || v < -0x80000000 || v > 0x7fffffff) throw new EncodeError(`${String(v)} is not an i32`);
    this.u32(((v << 1) ^ (v >> 31)) >>> 0);
  }

  /** 8 raw bytes; every NaN is written as the canonical quiet NaN. */
  f64(v: number): void {
    if (typeof v !== "number") throw new EncodeError(`${String(v)} is not a number`);
    this.reserve(8);
    if (v !== v) {
      this.view.setUint32(this.pos, 0, true);
      this.view.setUint32(this.pos + 4, 0x7ff80000, true);
    } else {
      this.view.setFloat64(this.pos, v, true);
    }
    this.pos += 8;
  }

  len(n: number): void {
    this.u32(n);
  }

  /** UTF-16 code units: the count, then 2 bytes each. */
  utf16(s: string): void {
    this.len(s.length);
    this.reserve(s.length * 2);
    for (let i = 0; i < s.length; i++) {
      this.view.setUint16(this.pos, s.charCodeAt(i), true);
      this.pos += 2;
    }
  }

  /** An inline byte string: the length, then the bytes. */
  bytes(b: Uint8Array): void {
    this.len(b.length);
    this.reserve(b.length);
    this.buf.set(b, this.pos);
    this.pos += b.length;
  }

  /** `n` bit-packed booleans, ceil(n/8) bytes (no count). */
  bits(n: number, at: (i: number) => boolean): void {
    this.reserve(Math.ceil(n / 8));
    let byte = 0;
    for (let i = 0; i < n; i++) {
      if (at(i)) byte |= 1 << (i & 7);
      if ((i & 7) === 7) {
        this.buf[this.pos++] = byte;
        byte = 0;
      }
    }
    if ((n & 7) !== 0) this.buf[this.pos++] = byte;
  }

  /** The bitmap of a struct. */
  bitmap(bits: boolean[]): void {
    this.bits(bits.length, (i) => bits[i]!);
  }

  /** A `list<bool>`: the count, then the bits. */
  bitList(items: boolean[]): void {
    this.len(items.length);
    this.bits(items.length, (i) => !!items[i]);
  }

  /** The message header: magic, schema hash, message id and generation. */
  header(magic: Uint8Array, hash: Uint8Array, message: number, generation: number): void {
    this.reserve(12);
    this.buf.set(magic, this.pos);
    this.pos += 4;
    this.buf.set(hash, this.pos);
    this.pos += 8;
    this.u32(message);
    this.u32(generation);
  }

  /** Appends what another writer holds. */
  raw(other: Writer): void {
    const b = other.finish();
    this.reserve(b.length);
    this.buf.set(b, this.pos);
    this.pos += b.length;
  }

  finish(): Uint8Array {
    return this.buf.slice(0, this.pos);
  }
}
