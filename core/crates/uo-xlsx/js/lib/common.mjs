// Shared plumbing of the uo-xlsx (Sheet) capture scripts: paths, the frozen TS checkout, the tracked samples and
// small argument helpers (the uo-pptx/js/lib/common.mjs pattern).
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = fileURLToPath(new URL('.', import.meta.url));
/** core/crates/uo-xlsx */
export const crateDir = resolve(here, '..', '..');
/** core */
export const coreDir = resolve(crateDir, '..', '..');
/** the repository (worktree) root */
export const repoDir = resolve(coreDir, '..');
export const capturesDir = join(crateDir, 'tests', 'captures');
/** big local runs (gitignored: core/target) */
export const bigDir = join(coreDir, 'target', 'sheet-cap');

export const DEFAULT_TS_ROOT = 'D:/Projects/OfficeNext/.claude/worktrees/ts-ref-092';

export function args(argv = process.argv.slice(2)) {
  const opt = (name, dflt) => {
    const i = argv.indexOf(name);
    return i >= 0 ? argv[i + 1] : dflt;
  };
  const flag = (name) => argv.includes(name);
  return { argv, opt, flag };
}

/** Frozen TS checkout: `--ts-root`, else `UO_TS_ROOT`, else the 0.9.2 reference worktree. */
export function tsRoot(a = args()) {
  return resolve(a.opt('--ts-root', process.env.UO_TS_ROOT || DEFAULT_TS_ROOT));
}

/** Commit of the frozen checkout (for the capture stamp), or `unknown`. */
export function tsCommit(root) {
  try { return execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(); } catch { return 'unknown'; }
}

/** `git status --porcelain` of the frozen checkout's sheet/ and shared/ is empty (the stamp says so). */
export function tsClean(root) {
  try {
    return execFileSync('git', ['-C', root, 'status', '--porcelain', '--', 'sheet/packages', 'shared'], { encoding: 'utf8' }).trim() === '' ? 'clean' : 'dirty';
  } catch { return 'unknown'; }
}

/** import() of a module of the frozen checkout by path relative to its root. */
export const importFrom = (root, rel) => import(pathToFileURL(join(root, rel)).href);

/** The git-tracked workbooks of `sheet/samples` (names), sorted. */
export function trackedSamples() {
  const out = execFileSync('git', ['-C', repoDir, 'ls-files', 'sheet/samples'], { encoding: 'utf8' });
  return out.split('\n').map((s) => s.trim()).filter((s) => /\.(xlsx|xlsm|xls)$/i.test(s)).map((s) => s.slice('sheet/samples/'.length)).sort();
}

export function readSample(rel) {
  return new Uint8Array(readFileSync(join(repoDir, 'sheet', 'samples', rel)));
}

/** A tab/newline-free one-line version of an error message for a capture value. */
export function oneLine(s, max = 160) {
  return String(s).replace(/[\t\r\n]+/g, ' ').replace(/=/g, ':').slice(0, max);
}

/** IEEE-754 bits of a number as 16 hex digits (NaN canonical as written by DataView). */
export function f64hex(x) {
  const v = new DataView(new ArrayBuffer(8));
  v.setFloat64(0, x);
  return v.getBigUint64(0).toString(16).padStart(16, '0');
}
