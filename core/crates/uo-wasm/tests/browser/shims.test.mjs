// Self-check of the browser runner's Buffer and SHA-256 shims against node:buffer and node:crypto (Node only):
//   node --test crates/uo-wasm/tests/browser/shims.test.mjs
// The painter parity runs through these shims in WebKit; a shim bug would hide or invent mismatches, so they are proven here.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash as nodeHash } from 'node:crypto';
import { Buffer as ShimBuffer } from './shims/buffer.mjs';
import { createHash as shimHash } from './shims/crypto.mjs';

let seed = 0x9e3779b9;
const rnd = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed; };
const bytes = (n) => Uint8Array.from({ length: n }, () => rnd() & 0xff);

test('SHA-256 equals node:crypto on 200 random inputs, with incremental updates and copy()', () => {
  for (let k = 0; k < 200; k++) {
    // lengths around the 55/56/63/64/119/120 padding edges and random ones up to a few blocks
    const edge = [0, 1, 55, 56, 57, 63, 64, 65, 119, 120, 121, 127, 128];
    const n = k < edge.length ? edge[k] : rnd() % 1000;
    const data = bytes(n);
    const a = nodeHash('sha256');
    const b = shimHash('sha256');
    let at = 0;
    while (at < n) {
      const step = Math.min(n - at, 1 + (rnd() % 150));
      a.update(data.subarray(at, at + step));
      b.update(data.subarray(at, at + step));
      at += step;
      if (rnd() % 4 === 0) assert.equal(b.copy().digest('hex'), a.copy().digest('hex'), `copy at ${at}/${n}`);
    }
    assert.equal(b.digest('hex'), a.digest('hex'), `digest of ${n} bytes`);
  }
  const s = 'aé€\u{1f600}';
  assert.equal(shimHash('sha256').update(s).digest('hex'), nodeHash('sha256').update(s).digest('hex'));
  assert.deepEqual([...shimHash('sha256').update(bytes(10)).digest()].length, 32);
});

test('Buffer shim: writers, readers, from, toString equal node:buffer on 200 random inputs', () => {
  for (let k = 0; k < 200; k++) {
    const a = Buffer.allocUnsafe(64).fill(0);
    const b = ShimBuffer.allocUnsafe(64).fill(0);
    const off = rnd() % 40;
    const big = (BigInt(rnd() >>> 1) << 32n) | BigInt(rnd());
    const dbl = (rnd() / 7919) * (rnd() % 2 ? -1 : 1);
    assert.equal(b.writeUInt32LE(0xdeadbeef, off), a.writeUInt32LE(0xdeadbeef, off));
    assert.equal(b.writeBigUInt64LE(big, off), a.writeBigUInt64LE(big, off));
    assert.equal(b.writeBigInt64LE(-big, off), a.writeBigInt64LE(-big, off));
    assert.equal(b.writeDoubleLE(dbl, off), a.writeDoubleLE(dbl, off));
    assert.equal(b.readBigUInt64LE(off), a.readBigUInt64LE(off));
    assert.equal(b.toString('hex'), a.toString('hex'));
    assert.equal(b.subarray(off, off + 8).toString('hex'), a.subarray(off, off + 8).toString('hex'));
    const text = String.fromCharCode(...Array.from({ length: rnd() % 12 }, () => 32 + (rnd() % 0x2000)));
    assert.equal(ShimBuffer.from(text, 'utf8').toString('hex'), Buffer.from(text, 'utf8').toString('hex'));
    assert.equal(ShimBuffer.from(text, 'utf8').toString('utf8'), Buffer.from(text, 'utf8').toString('utf8'));
    const u8 = bytes(rnd() % 40);
    assert.equal(ShimBuffer.from(u8.buffer, 0, u8.length).toString('hex'), Buffer.from(u8.buffer, 0, u8.length).toString('hex'));
    assert.equal(ShimBuffer.from(u8).toString('hex'), Buffer.from(u8).toString('hex'));
  }
  assert.equal(ShimBuffer.from('aGVsbG8=', 'base64').toString('utf8'), 'hello');
});
