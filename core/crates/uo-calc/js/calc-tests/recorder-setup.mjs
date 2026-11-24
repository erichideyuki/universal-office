// Recorder of the `calc-tests` capture (a vitest setup file; see vitest.config.mjs). Installs globalThis.__uoRec,
// which the rewritten TS modules call (recorder-plugin.mjs). Every OUTERMOST call of a wrapped entry point is kept
// as one JSON line in $UO_REC_DIR/<test file>.jsonl:
//   {"t": <test name>, "k": <call index in the file>, "fn": "<module>:<name>" | "<module>:<Class>.new" | "<module>:<Class>#<method>",
//    "self": <object id or null>, "args": [<canon>...], "ret": <canon>}  ("throw": "<Name>: <message>" instead of ret)
// Canonical values (`canon`): JSON numbers by shortest round trip except -0 / NaN / ±Infinity ({"$num"}), CalcError
// {"$err":code}, ArrayValue {"$arr":[rows, cols, [data]]}, RefValue {"$ref":[[sheet,r1,c1,r2,c2]...]},
// LambdaValue {"$lambda":[params], "js": true when the body is a JS function}, functions {"$fn": name},
// Map {"$map": [[k, v]...]}, Set {"$set": [...]}, typed arrays {"$bytes": length, "sha256"}, objects returned by
// an earlier recorded call (engines, contexts, pools) {"$obj": "#<k>"}, cycles {"$cycle": true}; object keys sorted.
// Thunks (lazy arguments) are functions with an `omitted` flag: {"$thunk": <canon of the value>, "omitted": bool}
// (evaluated once at record time, which a thunk allows: the evaluator does the same). "rnd": the Math.random values
// the call consumed (f64 hex), when any.
import { appendFileSync, mkdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { basename, join } from 'node:path';

// Host injection (decision C1): engines the tests build without `now`/`random` read the host clock and Math.random.
// The clock is frozen at 2025-01-15 12:00 UTC (TZ=UTC) and Math.random is a seeded xorshift32 (reseeded per test
// file); the values a recorded call consumed are kept in its line as "rnd" (f64 hex).
const FIXED = Date.UTC(2025, 0, 15, 12, 0, 0);
const RealDate = Date;
globalThis.Date = class extends RealDate {
  constructor(...x) { if (x.length === 0) super(FIXED); else super(...x); }
  static now() { return FIXED; }
};
let seed = 0x9e3779b9;
let rndLog = null;
Math.random = () => {
  seed ^= seed << 13; seed >>>= 0; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0;
  const x = seed / 4294967296;
  if (rndLog) rndLog.push(x);
  return x;
};
const f64 = (x) => { const v = new DataView(new ArrayBuffer(8)); v.setFloat64(0, x); return v.getBigUint64(0).toString(16).padStart(16, '0'); };

const dir = process.env.UO_REC_DIR;
if (!dir) throw new Error('UO_REC_DIR not set (run through capture-tests.mjs)');
mkdirSync(dir, { recursive: true });

let k = 0;
const ids = new WeakMap();
const lines = [];
const MAX = 1 << 16;
const testFile = () => {
  const p = expect.getState().testPath ?? 'unknown';
  return p.replace(/\\/g, '/').split('/sheet/packages/')[1] ?? basename(p);
};
const testName = () => expect.getState().currentTestName ?? '(module)';

function canon(v, seen = new Set()) {
  if (v === null) return null;
  switch (typeof v) {
    case 'number':
      if (Object.is(v, -0)) return { $num: '-0' };
      if (!Number.isFinite(v)) return { $num: String(v) };
      return v;
    case 'string': case 'boolean': return v;
    case 'undefined': return { $undef: true };
    case 'bigint': return { $bigint: String(v) };
    case 'symbol': return { $symbol: String(v.description) };
    case 'function':
      if ('omitted' in v) {
        let inner;
        try { inner = canon(v(), seen); } catch (e) { inner = { $throw: String(e?.message ?? e) }; }
        return { $thunk: inner, omitted: !!v.omitted };
      }
      return { $fn: v.name || 'anonymous' };
    default: break;
  }
  if (ids.has(v)) return { $obj: ids.get(v) };
  if (seen.has(v)) return { $cycle: true };
  const name = v.constructor?.name;
  if (name === 'CalcError') return { $err: v.code };
  if (ArrayBuffer.isView(v)) return { $bytes: v.byteLength, sha256: createHash('sha256').update(new Uint8Array(v.buffer, v.byteOffset, v.byteLength)).digest('hex') };
  seen.add(v);
  try {
    if (name === 'ArrayValue') return { $arr: [v.rows, v.cols, Array.from(v.data, (x) => canon(x, seen))] };
    if (name === 'RefValue') return { $ref: v.areas.map((a) => [a.sheet, a.r1, a.c1, a.r2, a.c2]) };
    if (name === 'LambdaValue') return { $lambda: [...v.params], js: typeof v.body === 'function' };
    if (Array.isArray(v)) return v.map((x) => canon(x, seen));
    if (v instanceof Map) return { $map: [...v].map(([a, b]) => [canon(a, seen), canon(b, seen)]) };
    if (v instanceof Set) return { $set: [...v].map((x) => canon(x, seen)) };
    if (v instanceof Error) return { $error: name, message: v.message };
    const out = {};
    if (name && name !== 'Object') out.$class = name;
    for (const key of Object.keys(v).sort()) out[key] = canon(v[key], seen);
    return out;
  } finally {
    seen.delete(v);
  }
}

const text = (x) => {
  const s = JSON.stringify(x);
  if (s === undefined) return 'null';
  if (s.length <= MAX) return s;
  return JSON.stringify({ $big: s.length, sha256: createHash('sha256').update(s).digest('hex') });
};

function record(fn, self, args, run) {
  if (rec.depth > 0) return run();
  const at = k++;
  const t = testName();
  // the arguments as they were BEFORE the call (some entry points mutate their input)
  const a = text(args.map((x) => canon(x)));
  rec.depth++;
  rndLog = [];
  let ret, threw;
  try {
    ret = run();
  } catch (e) {
    threw = e;
  } finally {
    rec.depth--;
  }
  const rnd = rndLog;
  rndLog = null;
  const line = { t, k: at, fn, self: self && ids.has(self) ? ids.get(self) : null, args: JSON.parse(a) };
  if (rnd.length) line.rnd = rnd.map(f64);
  if (threw !== undefined) line.throw = `${threw?.constructor?.name ?? 'Error'}: ${threw?.message ?? threw}`;
  else {
    // objects with behaviour (engines, contexts, pools; ASTs and values are data) get an id for later calls and
    // are not serialized
    if (ret !== null && typeof ret === 'object' && !ids.has(ret) && (fn.endsWith('.new') || /Context$|context$/.test(fn))) ids.set(ret, `#${at}`);
    line.ret = JSON.parse(text(canon(ret)));
  }
  lines.push(JSON.stringify(line));
  if (threw !== undefined) throw threw;
  return ret;
}

const rec = globalThis.__uoRec = {
  depth: 0,
  call(fn, impl, self, args) { return record(fn, null, args, () => impl.apply(self, args)); },
  wrapClass(cls, name, methods) {
    for (const m of methods) {
      const d = Object.getOwnPropertyDescriptor(cls.prototype, m);
      if (!d || typeof d.value !== 'function') continue;
      const impl = d.value;
      Object.defineProperty(cls.prototype, m, { ...d, value: { [m](...args) { return record(`${name}#${m}`, this, args, () => impl.apply(this, args)); } }[m] });
    }
    return new Proxy(cls, {
      construct(target, args, newTarget) {
        return record(`${name}.new`, null, args, () => Reflect.construct(target, args, newTarget));
      },
    });
  },
};

afterAll(() => {
  const file = testFile().replace(/[\\/]/g, '__');
  appendFileSync(join(dir, `${file}.jsonl`), lines.length ? lines.join('\n') + '\n' : '');
  lines.length = 0;
});
