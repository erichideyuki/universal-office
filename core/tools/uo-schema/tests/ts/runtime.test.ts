// The TypeScript runtime of the codec (core/crates/uo-codec/js/runtime.ts): the same boundaries
// and rejections the Rust runtime tests cover, so the two implementations of the wire format agree.
//   node --test core/tools/uo-schema/tests/ts/runtime.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import { DecodeError, EncodeError, Reader, SymTable, Writer, bitAt, DEFAULT_LIMITS } from "../../../../crates/uo-codec/js/runtime.ts";

const write = (f: (w: Writer) => void): Uint8Array => {
  const w = new Writer(2);
  f(w);
  return w.finish();
};
const reader = (b: number[] | Uint8Array): Reader => new Reader(b instanceof Uint8Array ? b : new Uint8Array(b));
const code = (f: () => unknown): string => {
  try {
    f();
  } catch (e) {
    if (e instanceof DecodeError) return e.code;
    throw e;
  }
  return "ok";
};

test("u32 varints at every boundary, shortest form, same bytes as Rust", () => {
  for (const v of [0, 1, 0x7f, 0x80, 0x3fff, 0x4000, 0x1fffff, 0x200000, 0x0fffffff, 0x10000000, 0xfffffffe, 0xffffffff]) {
    const b = write((w) => w.u32(v));
    assert.equal(reader(b).u32(), v);
  }
  assert.deepEqual([...write((w) => w.u32(300))], [0xac, 0x02]);
  assert.deepEqual([...write((w) => w.u32(0xffffffff))], [0xff, 0xff, 0xff, 0xff, 0x0f]);
  for (const bad of [[0x80, 0x00], [0xff, 0x00], [0x80, 0x80, 0x80, 0x80, 0x00], [0x80, 0x80, 0x80, 0x80, 0x80, 0x01], [0xff, 0xff, 0xff, 0xff, 0x10]]) {
    assert.equal(code(() => reader(bad).u32()), "BadVarint", String(bad));
  }
  assert.equal(code(() => reader([0x80]).u32()), "Truncated");
  assert.equal(code(() => reader([0x80, 0x80, 0x04]).u16()), "BadVarint");
});

test("i32 zigzag", () => {
  for (const v of [0, 1, -1, 63, -64, 64, -65, 0x7fffffff, -0x80000000]) assert.equal(reader(write((w) => w.i32(v))).i32(), v);
  assert.deepEqual([...write((w) => w.i32(-1))], [1]);
  assert.deepEqual([...write((w) => w.i32(1))], [2]);
  assert.deepEqual([...write((w) => w.i32(-0x80000000))], [0xff, 0xff, 0xff, 0xff, 0x0f]);
});

test("values out of range do not encode", () => {
  for (const f of [() => write((w) => w.u32(-1)), () => write((w) => w.u32(2 ** 32)), () => write((w) => w.u32(1.5)), () => write((w) => w.u8(256)), () => write((w) => w.u16(65536)), () => write((w) => w.i32(2 ** 31))]) {
    assert.throws(f, EncodeError);
  }
});

test("f64 keeps -0 and the infinities, canonicalizes NaN, refuses other NaNs", () => {
  for (const v of [0, -0, 1.5, Number.MIN_VALUE, Number.MAX_VALUE, Infinity, -Infinity]) {
    const got = reader(write((w) => w.f64(v))).f64();
    assert.ok(Object.is(got, v), String(v));
  }
  assert.deepEqual([...write((w) => w.f64(NaN))], [0, 0, 0, 0, 0, 0, 0xf8, 0x7f]);
  assert.ok(Number.isNaN(reader([0, 0, 0, 0, 0, 0, 0xf8, 0x7f]).f64()));
  assert.equal(code(() => reader([1, 0, 0, 0, 0, 0, 0xf8, 0x7f]).f64()), "NonCanonicalNaN");
  assert.equal(code(() => reader([0, 0, 0, 0, 0, 0, 0xf8, 0xff]).f64()), "NonCanonicalNaN");
  assert.equal(code(() => reader([0, 0, 0, 0, 0, 0, 0xf8]).f64()), "Truncated");
});

test("strings keep every code unit, including lone surrogates and long strings", () => {
  const s = String.fromCharCode(0, 0x41, 0xd800, 0xdc00, 0xdbff, 0xdfff, 0xffff, 0xfeff);
  assert.equal(reader(write((w) => w.utf16(s))).utf16(), s);
  assert.deepEqual([...write((w) => w.utf16(""))], [0]);
  const long = "x".repeat(10000) + String.fromCharCode(0xd800);
  assert.equal(reader(write((w) => w.utf16(long))).utf16(), long);
  assert.equal(code(() => reader([3, 0x41, 0]).utf16()), "Truncated");
});

test("bitmaps and bit lists are bit-packed with zero padding", () => {
  const bits = [true, false, true, true, false, false, false, false, true];
  assert.deepEqual([...write((w) => w.bitmap(bits))], [0b00001101, 0b00000001]);
  const r = reader([0b00001101, 0b00000001]);
  const bm = r.bitmap(9);
  assert.deepEqual(bits.map((_, i) => bitAt(bm, i)), bits);
  assert.equal(code(() => reader([0b00001101, 0b00000011]).bitmap(9)), "NonZeroPadding");
  const list = [true, true, false, true, false, false, true, true, true, false];
  assert.deepEqual([...write((w) => w.bitList(list))], [10, 0b11001011, 0b00000001]);
  assert.deepEqual(reader(write((w) => w.bitList(list))).bitList(), list);
  assert.equal(code(() => reader([9, 0xff, 0x02]).bitList()), "NonZeroPadding");
  assert.equal(write((w) => w.bitmap([])).length, 0);
});

test("counts are checked against the bytes and the limits", () => {
  assert.equal(code(() => reader([0xe8, 0x07, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).listLen(16)), "Truncated");
  assert.equal(reader([0xe8, 0x07, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).listLen(0), 1000);
  const small = new Reader(new Uint8Array([100, 0, 0]), { ...DEFAULT_LIMITS, maxListLen: 10 });
  assert.equal(code(() => small.listLen(0)), "Limit");
  const total = new Reader(new Uint8Array(30).fill(10, 0, 2), { ...DEFAULT_LIMITS, maxTotal: 15 });
  assert.equal(total.listLen(1), 10);
  assert.equal(code(() => total.listLen(1)), "Limit");
  const deep = new Reader(new Uint8Array(0), { ...DEFAULT_LIMITS, maxDepth: 2 });
  deep.enter();
  deep.enter();
  assert.equal(code(() => deep.enter()), "Limit");
});

test("header, table header and ids", () => {
  const magic = new Uint8Array([65, 66, 67, 68]);
  const hash = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]);
  const b = write((w) => {
    w.header(magic, hash, 3, 9);
    w.u32(2);
    w.u32(1);
    w.u32(2);
  });
  const r = reader(b);
  assert.deepEqual(r.header(magic, hash), { message: 3, generation: 9 });
  assert.equal(r.tableHeader(0, "t", 2, 8), 1);
  assert.equal(r.ref(0, "t"), 2);
  assert.equal(code(() => reader([3]).ref(0, "t")), "BadRef");
  const r2 = reader(b);
  r2.header(magic, hash);
  assert.equal(code(() => r2.tableHeader(0, "t", 5, 8)), "TableGap");
  assert.equal(code(() => reader(b).header(new Uint8Array([65, 66, 67, 69]), hash)), "BadMagic");
  assert.equal(code(() => reader(b).header(magic, new Uint8Array(8))), "VersionMismatch");
  assert.equal(code(() => reader(b.subarray(0, 6)).header(magic, hash)), "Truncated");
});

test("the symbol table interns, keeps the first id of a repeated string and truncates", () => {
  const t = new SymTable();
  assert.equal(t.intern("a"), 0);
  assert.equal(t.intern("b"), 1);
  assert.equal(t.intern("a"), 0);
  t.push("a");
  assert.equal(t.length, 3);
  assert.equal(t.intern("a"), 0);
  t.truncate(1);
  assert.deepEqual(t.list, ["a"]);
  assert.equal(t.intern("b"), 1);
  t.truncate(0);
  assert.equal(t.intern("a"), 0);
});

test("the writer grows past its capacity and keeps appending after raw", () => {
  const a = new Writer(1);
  for (let i = 0; i < 1000; i++) a.u32(i);
  const b = new Writer(1);
  b.u8(7);
  b.raw(a);
  const out = b.finish();
  const r = reader(out);
  assert.equal(r.u8(), 7);
  for (let i = 0; i < 1000; i++) assert.equal(r.u32(), i);
  r.end();
});

test("trailing bytes", () => {
  assert.equal(code(() => reader([0, 0]).end()), "TrailingBytes");
});
