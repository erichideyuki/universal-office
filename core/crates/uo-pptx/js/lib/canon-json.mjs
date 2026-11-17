// Canonical JSON of a TS model value, the form the Rust side must reproduce byte for byte (the
// capture stores the SHA-256 of this text per presentation / slide; sidecars hold the text).
//
// Rules (also in core/docs/modules/uo-pptx-captures.md):
//   * objects: own enumerable string keys in UTF-16 code-unit order, `{"k":v,...}`, no spaces;
//     a key whose value is `undefined` (or a function / symbol) is omitted (and listed in `dropped`)
//   * arrays: `[a,b]`; an `undefined` element is `null` (listed)
//   * numbers: JavaScript's shortest round-trip decimal (`String(x)`), `-0` for negative zero;
//     NaN / Infinity / -Infinity are `{"$num":"NaN"|"Infinity"|"-Infinity"}`
//   * strings: `JSON.stringify` (lone surrogates are written as `\udXXX` escapes); a string longer
//     than LONG_TEXT UTF-16 units is replaced by `{"$text":<length in UTF-16 units>,"sha256":<hex of its UTF-8>}`
//     (lone surrogates hash as U+FFFD, like Buffer)
//   * Uint8Array / Buffer: `{"$bytes":<length>,"sha256":<hex>}` (the bytes themselves are not stored)
//   * Map: `{"$map":[[k,v],...]}` in insertion order; Set: `{"$set":[...]}`
//   * shared references are written in full at every occurrence (identity is not recorded);
//     a cycle throws
import { createHash } from 'node:crypto';

export const LONG_TEXT = 4096;

const cmp = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

/**
 * `dropped`: an optional Map<string, number> that receives `<what>` -> occurrence count
 * (e.g. `undefined-key`, `function`, `undefined-element`, `bytes->digest`, `long-text->digest`).
 */
export function canonJson(value, dropped = new Map()) {
  const note = (what) => dropped.set(what, (dropped.get(what) ?? 0) + 1);
  const stack = new Set();
  const out = [];
  const go = (v) => {
    if (v === null) return void out.push('null');
    switch (typeof v) {
      case 'boolean': return void out.push(v ? 'true' : 'false');
      case 'number':
        if (Number.isNaN(v)) return void out.push('{"$num":"NaN"}');
        if (v === Infinity) return void out.push('{"$num":"Infinity"}');
        if (v === -Infinity) return void out.push('{"$num":"-Infinity"}');
        return void out.push(Object.is(v, -0) ? '-0' : String(v));
      case 'bigint': return void out.push(`{"$bigint":"${v}"}`);
      case 'string':
        if (v.length > LONG_TEXT) {
          note('long-text->digest');
          return void out.push(`{"$text":${v.length},"sha256":"${createHash('sha256').update(Buffer.from(v, 'utf8')).digest('hex')}"}`);
        }
        return void out.push(JSON.stringify(v));
      case 'undefined': note('undefined-element'); return void out.push('null');
      case 'function': case 'symbol': note(typeof v); return void out.push('null');
      default: break;
    }
    if (v instanceof Uint8Array) {
      note('bytes->digest');
      return void out.push(`{"$bytes":${v.length},"sha256":"${createHash('sha256').update(v).digest('hex')}"}`);
    }
    if (stack.has(v)) throw new Error('cycle in model value');
    stack.add(v);
    if (Array.isArray(v)) {
      out.push('[');
      v.forEach((x, i) => { if (i) out.push(','); go(x); });
      out.push(']');
    } else if (v instanceof Map) {
      out.push('{"$map":[');
      let i = 0;
      for (const [k, x] of v) { if (i++) out.push(','); out.push('['); go(k); out.push(','); go(x); out.push(']'); }
      out.push(']}');
    } else if (v instanceof Set) {
      out.push('{"$set":[');
      let i = 0;
      for (const x of v) { if (i++) out.push(','); go(x); }
      out.push(']}');
    } else {
      out.push('{');
      let first = true;
      for (const k of Object.keys(v).sort(cmp)) {
        const x = v[k];
        if (x === undefined || typeof x === 'function' || typeof x === 'symbol') { note(x === undefined ? 'undefined-key' : `${typeof x}-key`); continue; }
        if (!first) out.push(',');
        first = false;
        out.push(JSON.stringify(k), ':');
        go(x);
      }
      out.push('}');
    }
    stack.delete(v);
  };
  go(value);
  return out.join('');
}

/** `len:sha256[0..32]` of a text (UTF-8) — the form used by capture facets. */
export function textDigest(text) {
  const b = Buffer.from(text, 'utf8');
  return `${b.length}:${createHash('sha256').update(b).digest('hex').slice(0, 32)}`;
}

/** `len:sha256[0..16]` of bytes (inputs). */
export function bytesDigest(u8) {
  return `${u8.length}:${createHash('sha256').update(u8).digest('hex').slice(0, 16)}`;
}
