// Shared plumbing of the uo-calc capture scripts (plan section 4.3 "Calc", record core/docs/modules/uo-calc.md
// "Captures"): the frozen TS checkout, the calc dist, the canonical text of a calc value, the injected clock and
// random source (decision C1) and the capture stamp.
//
// The calc captures run with TZ=UTC (set by each script before any import that reads the zone) and with the clock
// fixed at NOW_SERIAL (2025-01-01 12:00, the TS test default); random values come from a seeded xorshift32 and
// are recorded in call order (RandomSource::Sequence on the Rust side).
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Rng } from '../../../uo-testkit/js/testkit.mjs';

const here = fileURLToPath(new URL('.', import.meta.url));
/** core/crates/uo-calc */
export const crateDir = resolve(here, '..', '..');
export const coreDir = resolve(crateDir, '..', '..');
export const repoDir = resolve(coreDir, '..');
export const capturesDir = join(crateDir, 'tests', 'captures');
/** big local runs (gitignored) */
export const bigDir = join(coreDir, 'target', 'sheet-cap');
/** the oracle bank, read from this worktree (the same files as the frozen checkout; the capture stamps their digest) */
export const oracleDir = join(repoDir, 'sheet', 'packages', 'calc', 'test', 'oracle');

export const DEFAULT_TS_ROOT = 'D:/Projects/OfficeNext/.claude/worktrees/ts-ref-092';
/** "now" of every calc capture: 45658.5 = 2025-01-01 12:00 (the default of the TS testing.ts mock). */
export const NOW_SERIAL = 45658.5;

export function args(argv = process.argv.slice(2)) {
  const opt = (name, dflt) => {
    const i = argv.indexOf(name);
    return i >= 0 ? argv[i + 1] : dflt;
  };
  const flag = (name) => argv.includes(name);
  return { argv, opt, flag };
}

export function tsRoot(a = args()) {
  return resolve(a.opt('--ts-root', process.env.UO_TS_ROOT || DEFAULT_TS_ROOT));
}

export function tsCommit(root) {
  try { return execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(); } catch { return 'unknown'; }
}

export function tsClean(root) {
  try {
    return execFileSync('git', ['-C', root, 'status', '--porcelain', '--', 'sheet/packages/calc'], { encoding: 'utf8' }).trim() === '' ? 'clean' : 'dirty';
  } catch { return 'unknown'; }
}

/** The calc dist of the frozen checkout: { calc (index.js), testing (testing.js) }. */
export async function loadCalc(root) {
  const base = join(root, 'sheet', 'packages', 'calc', 'dist');
  const calc = await import(pathToFileURL(join(base, 'index.js')).href);
  const testing = await import(pathToFileURL(join(base, 'testing.js')).href);
  const engine = await import(pathToFileURL(join(base, 'engine.js')).href);
  return { calc, testing, engine };
}

/** The stamp metas every calc capture carries. */
export function stamp(writer, root, extra = {}) {
  writer.meta('ts-root-commit', tsCommit(root));
  writer.meta('ts-root-calc', tsClean(root));
  writer.meta('tz', process.env.TZ ?? 'unset');
  writer.meta('now-serial', NOW_SERIAL);
  for (const [k, v] of Object.entries(extra)) writer.meta(k, v);
}

export function f64hex(x) {
  const v = new DataView(new ArrayBuffer(8));
  v.setFloat64(0, x);
  return v.getBigUint64(0).toString(16).padStart(16, '0');
}

export function hexf64(h) {
  const v = new DataView(new ArrayBuffer(8));
  v.setBigUint64(0, BigInt(`0x${h}`));
  return v.getFloat64(0);
}

/**
 * Canonical text of a calc value (the form every calc capture compares; one line, no tab):
 *   _                 empty (null)
 *   n<16 hex>         number by its IEEE bits (-0 and NaN payloads kept)
 *   s<JSON string>    text (JSON.stringify: lone surrogates as \udXXX escapes)
 *   b1 | b0           boolean
 *   e<code>           error (#DIV/0!, #N/A, ...)
 *   a<r>x<c>[v,v;v,v] array, rows separated by `;`
 *   r<JSON areas>     reference (RefValue.areas)
 *   L<n>              lambda with n parameters
 *   ?<typeof>         anything else (a TS bug if it shows)
 */
export function makeValueText(calc) {
  const { CalcError, ArrayValue, RefValue, LambdaValue } = calc;
  const vt = (v) => {
    if (v === null || v === undefined) return v === null ? '_' : '?undefined';
    switch (typeof v) {
      case 'number': return `n${f64hex(v)}`;
      case 'string': return `s${JSON.stringify(v)}`;
      case 'boolean': return v ? 'b1' : 'b0';
      default: break;
    }
    if (v instanceof CalcError) return `e${v.code}`;
    if (v instanceof ArrayValue) {
      const rows = [];
      for (let r = 0; r < v.rows; r++) {
        const row = [];
        for (let c = 0; c < v.cols; c++) row.push(vt(v.get(r, c)));
        rows.push(row.join(','));
      }
      return `a${v.rows}x${v.cols}[${rows.join(';')}]`;
    }
    if (v instanceof RefValue) return `r${JSON.stringify(v.areas.map((a) => [a.sheet, a.r1, a.c1, a.r2, a.c2]))}`;
    if (v instanceof LambdaValue) return `L${v.params?.length ?? '?'}`;
    return `?${typeof v}:${v?.constructor?.name ?? ''}`;
  };
  return vt;
}

/** A value text that may be long: itself up to `max` UTF-16 units, else `#<len>:<sha256[0..32]>` (full text in a sidecar). */
export function clip(text, max = 512) {
  if (text.length <= max) return text;
  const b = Buffer.from(text, 'utf8');
  return `#${b.length}:${createHash('sha256').update(b).digest('hex').slice(0, 32)}`;
}

export const sha = (s) => createHash('sha256').update(typeof s === 'string' ? Buffer.from(s, 'utf8') : s).digest('hex');
export const digest = (s) => { const b = Buffer.from(s, 'utf8'); return `${b.length}:${sha(b).slice(0, 32)}`; };

/**
 * A recorded random source (RandomSource::Sequence): xorshift32 (uo-testkit Rng) mapped to [0, 1) by `next() /
 * 2^32`; every value handed out is kept in `seq`.
 */
export class RecordedRandom {
  constructor(seed) { this.rng = new Rng(seed >>> 0); this.seq = []; }
  next() { const x = this.rng.next() / 4294967296; this.seq.push(x); return x; }
  /** the sequence as `count:digest` + the hex list (for sidecars) */
  text() { return this.seq.map(f64hex).join(','); }
  take() { const s = this.seq; this.seq = []; return s; }
}

/** One-line error text for a capture value. */
export function oneLine(s, max = 160) {
  return String(s).replace(/[\t\r\n]+/g, ' ').slice(0, max);
}
