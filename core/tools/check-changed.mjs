#!/usr/bin/env node
// Local lint gate over what changed: clippy -D warnings (native and wasm32, --all-targets --all-features) and the
// ban scripts, only on the crates touched since a git base plus every workspace crate that depends on them (normal,
// dev or build dependency, transitively). The whole-workspace run stays the CI's job and the milestone check's.
//
//   node tools/check-changed.mjs [base]        base = a git ref (default HEAD: uncommitted + untracked changes)
//   node tools/check-changed.mjs --all         every crate (same as CI)
//   options: --native-only, --wasm-only, --dry-run (print the crates and commands), --jobs N (default 4)
//
// A change outside crates/ that affects every crate (Cargo.toml, Cargo.lock, clippy.toml, rust-toolchain*,
// .cargo/) selects the whole workspace. Run from anywhere inside the repository.
import { execFileSync, spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const core = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const flag = (f) => args.includes(f);
const jobsAt = args.indexOf('--jobs');
const jobs = jobsAt >= 0 ? args[jobsAt + 1] : '4';
const base = args.find((a, i) => !a.startsWith('--') && args[i - 1] !== '--jobs') ?? 'HEAD';

const git = (...a) => execFileSync('git', a, { cwd: core, encoding: 'utf8' }).split('\n').filter(Boolean);
const top = git('rev-parse', '--show-toplevel')[0];
const changed = [...git('diff', '--name-only', base, '--', '.'), ...git('ls-files', '--others', '--exclude-standard', '--', '.')]
  .map((f) => path.resolve(top, f));

const meta = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], { cwd: core, encoding: 'utf8', maxBuffer: 1 << 28 }));
const crates = meta.packages.map((p) => ({ name: p.name, dir: path.dirname(p.manifest_path), deps: p.dependencies.filter((d) => d.path).map((d) => d.name) }));
const names = new Set(crates.map((c) => c.name));

const global = /^(Cargo\.(toml|lock)|clippy\.toml|rust-toolchain(\.toml)?|\.cargo[\\/])/;
let selected;
if (flag('--all') || changed.some((f) => global.test(path.relative(core, f)))) {
  selected = new Set(names);
} else {
  selected = new Set();
  for (const f of changed) {
    const owner = crates.filter((c) => f.startsWith(c.dir + path.sep)).sort((a, b) => b.dir.length - a.dir.length)[0];
    if (owner) selected.add(owner.name);
  }
  // reverse dependencies, transitively
  for (let grew = true; grew; ) {
    grew = false;
    for (const c of crates) {
      if (!selected.has(c.name) && c.deps.some((d) => selected.has(d))) { selected.add(c.name); grew = true; }
    }
  }
}

const list = [...selected].sort();
console.log(`base ${base}: ${changed.length} changed files → ${list.length} crate(s): ${list.join(' ') || '(none)'}`);
if (list.length === 0) process.exit(0);

const pkgs = list.flatMap((n) => ['-p', n]);
const steps = [];
if (!flag('--wasm-only')) steps.push(['cargo', ['clippy', '-j', jobs, ...pkgs, '--all-targets', '--all-features', '--', '-D', 'warnings']]);
if (!flag('--native-only')) steps.push(['cargo', ['clippy', '-j', jobs, ...pkgs, '--all-targets', '--all-features', '--target', 'wasm32-unknown-unknown', '--', '-D', 'warnings']]);
if (changed.some((f) => /clippy\.toml$|tests[\\/]clippy-ban/.test(f)) || flag('--all')) steps.push(['bash', ['tests/clippy-ban/check.sh']]);
steps.push(['bash', ['tests/float-casts.sh'], { CRATES: list.join(' ') }]);
steps.push(['bash', ['tests/grep-bans.sh']]);

let failed = 0;
for (const [cmd, a, env] of steps) {
  console.log(`\n$ ${env ? Object.entries(env).map(([k, v]) => `${k}="${v}" `).join('') : ''}${cmd} ${a.join(' ')}`);
  if (flag('--dry-run')) continue;
  const r = spawnSync(cmd, a, { cwd: core, stdio: 'inherit', env: { ...process.env, ...env }, shell: false });
  if (r.status !== 0) { failed++; console.log(`→ FAILED (${r.status})`); }
}
console.log(failed ? `\n${failed} step(s) failed` : '\nall steps passed');
process.exit(failed ? 1 : 0);
