// Shared plumbing of the uo-pptx capture scripts: the frozen TS checkout, the tracked samples,
// the generated groups and small argument helpers.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = fileURLToPath(new URL('.', import.meta.url));
/** core/crates/uo-pptx */
export const crateDir = resolve(here, '..', '..');
/** the repository (worktree) root */
export const repoDir = resolve(crateDir, '..', '..', '..');
export const capturesDir = join(crateDir, 'tests', 'captures');

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

/** The built slides-core of the frozen checkout (`dist/index.js`). */
export async function loadSlidesCore(root) {
  return import(pathToFileURL(join(root, 'slides', 'packages', 'slides-core', 'dist', 'index.js')).href);
}

/** Commit of the frozen checkout (for the capture header), or `unknown`. */
export function tsCommit(root) {
  try { return execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(); } catch { return 'unknown'; }
}

/** The git-tracked files of `slides/samples` (repo-relative to `slides/samples`), sorted. */
export function trackedSamples() {
  const out = execFileSync('git', ['-C', repoDir, 'ls-files', 'slides/samples'], { encoding: 'utf8' });
  return out.split('\n').map((s) => s.trim()).filter((s) => /\.(pptx|ppt)$/i.test(s)).map((s) => s.slice('slides/samples/'.length)).sort();
}

export function readSample(rel) {
  return new Uint8Array(readFileSync(join(repoDir, 'slides', 'samples', rel)));
}

/** A tab/newline-free one-line version of an error message for a capture value. */
export function oneLine(s, max = 160) {
  return String(s).replace(/[\t\r\n]+/g, ' ').replace(/=/g, ':').slice(0, max);
}
