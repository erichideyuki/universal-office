#!/usr/bin/env node
// Third-party licence notices of what the Rust core ships, generated into core/licenses/.
//
// Five artifacts carry compiled third-party Rust code:
//   wasm    chunks/uo-write.wasm of @universal-office/write and @universal-office/document-viewer
//           (cargo tree -p uo-wasm --features write --target wasm32-unknown-unknown)
//   wasm-editor  chunks/uo-write-editor.wasm of @universal-office/write (the editor, engine="rust"; the session build)
//           (cargo tree -p uo-wasm --features session --target wasm32-unknown-unknown)
//   wasm-slides  chunks/uo-slides.wasm of @universal-office/slides-viewer and @universal-office/slides (engine="rust", B7)
//           (cargo tree -p uo-wasm --features slides --target wasm32-unknown-unknown)
//   wasm-slides-ppt  chunks/uo-slides-ppt.wasm, the lazily loaded .ppt converter of the two Slides packages
//           (cargo tree -p uo-wasm --features slides-ppt --target wasm32-unknown-unknown)
//   native  uo_ffi, the native library of the NuGet package UniversalOffice.Docx
//           (cargo tree -p uo-ffi --target all)
//
// Usage (run from core/, or anywhere: paths are resolved from this file):
//   node tools/licenses/gen-notices.mjs           regenerate core/licenses/
//   node tools/licenses/gen-notices.mjs --check   rebuild in memory and compare with the committed files (CI)
//
// Inputs: cargo tree/metadata (the crates actually linked), Cargo.lock, the licence files at the root of each
// registry crate in ~/.cargo/registry/src (no network), the texts kept in the workspace crates (uo-jsnum, uo-opc,
// uo-fonts data) and the Rust toolchain's own notices (<sysroot>/share/doc/rust). The licence rule is the
// [licenses] allow list of core/deny.toml (override for tests: UO_LICENSE_ALLOW="MIT,Apache-2.0").
//
// Exported (cargo-free): verifyNotices(dir, lockPath, artifact) used by tools/build-pkg.mjs and the NuGet pack check.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const CORE = path.resolve(here, '..', '..');
const OUT = path.join(CORE, 'licenses');
const LICENSE_RE = /^(LICEN[CS]E|COPYING|COPYRIGHT|NOTICE)([-._].*)?$/i;
/** Files that depend on the toolchain that ran the generator: checked for presence only. */
const TOOLCHAIN_PREFIXES = ['texts/rust-std/', 'texts/unicode/'];

const lf = (s) => s.replace(/\r\n/g, '\n');

/** Registry packages of a Cargo.lock as { name: version } (name@version when a name repeats). */
function lockRegistry(lockPath) {
  const text = lf(readFileSync(lockPath, 'utf8'));
  const out = {};
  for (const block of text.split('[[package]]').slice(1)) {
    const name = /^name = "([^"]+)"/m.exec(block)?.[1];
    const version = /^version = "([^"]+)"/m.exec(block)?.[1];
    const source = /^source = "([^"]+)"/m.exec(block)?.[1];
    if (name && version && source?.startsWith('registry+')) out[out[name] ? `${name}@${version}` : name] = version;
  }
  return out;
}

/** Files of one artifact listed in notices.json, plus the two index files; throws on any problem. */
export function verifyNotices(dir, lockPath, artifact) {
  const jsonPath = path.join(dir, 'notices.json');
  if (!existsSync(jsonPath) || !existsSync(path.join(dir, 'THIRD-PARTY-NOTICES.md'))) {
    throw new Error(`notices missing in ${dir} (THIRD-PARTY-NOTICES.md, notices.json): run node core/tools/licenses/gen-notices.mjs`);
  }
  const notices = JSON.parse(readFileSync(jsonPath, 'utf8'));
  const want = lockRegistry(lockPath);
  const have = notices.lock ?? {};
  const diff = [...new Set([...Object.keys(want), ...Object.keys(have)])].filter((k) => want[k] !== have[k]);
  if (diff.length) throw new Error(`notices are stale against ${path.basename(lockPath)}: ${diff.slice(0, 5).join(', ')}${diff.length > 5 ? ', ...' : ''}; run node core/tools/licenses/gen-notices.mjs`);
  const list = notices.artifacts?.[artifact];
  if (!Array.isArray(list) || list.length === 0) throw new Error(`notices.json has no components for artifact "${artifact}"`);
  const files = new Set(['THIRD-PARTY-NOTICES.md', 'notices.json']);
  for (const c of list) {
    if (!c.files?.length) throw new Error(`component ${c.component} has no licence files`);
    for (const f of c.files) files.add(f);
  }
  for (const f of files) if (!existsSync(path.join(dir, f))) throw new Error(`listed notice file missing: ${f}`);
  return [...files];
}

// ---- generation (needs cargo) ----

function cargo(args) {
  return execFileSync('cargo', args, { cwd: CORE, encoding: 'utf8', maxBuffer: 1 << 28, stdio: ['ignore', 'pipe', 'inherit'] });
}

/** Linked crates of one cargo tree: [{ name, version, license, features, workspace }]. */
function tree(args) {
  const map = new Map();
  for (const raw of cargo(['tree', ...args, '-e', 'normal', '--prefix', 'none', '--format', '{p}|{l}|{f}']).split('\n')) {
    const line = raw.replace(/ \(\*\)$/, '').trim();
    if (!line) continue;
    const [p, license, features] = line.split('|');
    const m = /^(\S+) v(\S+)(?: \((.+)\))?$/.exec(p);
    if (!m) throw new Error(`gen-notices: cannot read cargo tree line: ${raw}`);
    const key = `${m[1]}@${m[2]}`;
    const cur = map.get(key) ?? { name: m[1], version: m[2], license, features: new Set(), workspace: Boolean(m[3]) };
    for (const f of (features ?? '').split(',').filter(Boolean)) cur.features.add(f);
    map.set(key, cur);
  }
  return map;
}

function allowList() {
  if (process.env.UO_LICENSE_ALLOW) return process.env.UO_LICENSE_ALLOW.split(',').map((s) => s.trim()).filter(Boolean);
  const toml = lf(readFileSync(path.join(CORE, 'deny.toml'), 'utf8'));
  const m = /\[licenses\][^[]*?allow\s*=\s*\[([^\]]*)\]/s.exec(toml);
  if (!m) throw new Error('gen-notices: core/deny.toml has no [licenses] allow list');
  return [...m[1].matchAll(/"([^"]+)"/g)].map((x) => x[1]);
}

/** `A OR B` / `A AND B` (no parentheses, no WITH): one OR-alternative whose AND-terms are all allowed. */
function licenceOk(expr, allow) {
  if (/[()]|\bWITH\b/.test(expr)) throw new Error(`gen-notices: licence expression too complex for the checker: ${expr}`);
  const set = new Set(allow);
  return expr.split(/\s+OR\s+/).some((alt) => alt.split(/\s+AND\s+/).every((t) => set.has(t.trim())));
}

function build() {
  const allow = allowList();
  const sysroot = execFileSync('rustc', ['--print', 'sysroot'], { encoding: 'utf8' }).trim();
  const rustc = execFileSync('rustc', ['--version'], { encoding: 'utf8' }).trim();
  const stdDoc = path.join(sysroot, 'share', 'doc', 'rust');
  const meta = JSON.parse(cargo(['metadata', '--format-version', '1', '--all-features']));
  const dirOf = new Map(meta.packages.map((p) => [`${p.name}@${p.version}`, path.dirname(p.manifest_path)]));

  const artifacts = {
    wasm: tree(['-p', 'uo-wasm', '--features', 'write', '--target', 'wasm32-unknown-unknown']),
    'wasm-editor': tree(['-p', 'uo-wasm', '--features', 'session', '--target', 'wasm32-unknown-unknown']),
    'wasm-slides': tree(['-p', 'uo-wasm', '--features', 'slides', '--target', 'wasm32-unknown-unknown']),
    'wasm-slides-ppt': tree(['-p', 'uo-wasm', '--features', 'slides-ppt', '--target', 'wasm32-unknown-unknown']),
    native: tree(['-p', 'uo-ffi', '--target', 'all']),
  };
  /** relative path (posix) -> Buffer */
  const files = new Map();
  const put = (rel, buf) => files.set(rel, buf);
  const copyDir = (srcDir, destRel, pick) => {
    const got = [];
    for (const f of readdirSync(srcDir).sort()) {
      const full = path.join(srcDir, f);
      if (!statSync(full).isFile() || !pick(f)) continue;
      put(`${destRel}/${f}`, Buffer.from(lf(readFileSync(full, 'latin1')), 'latin1'));
      got.push(`${destRel}/${f}`);
    }
    return got;
  };
  const stdFiles = [];
  for (const f of ['COPYRIGHT-library.html']) stdFiles.push([`texts/rust-std/${f}`, path.join(stdDoc, f)]);
  for (const f of ['MIT.txt', 'Apache-2.0.txt']) stdFiles.push([`texts/rust-std/${f}`, path.join(stdDoc, 'licenses', f)]);
  for (const [rel, src] of stdFiles) {
    if (!existsSync(src)) throw new Error(`gen-notices: toolchain notice missing: ${src} (rustup component rust-docs or the minimal profile?)`);
    put(rel, Buffer.from(lf(readFileSync(src, 'latin1')), 'latin1'));
  }

  const bad = [];
  const out = { wasm: [], native: [], 'wasm-editor': [], 'wasm-slides': [], 'wasm-slides-ppt': [] };
  const workspaceTexts = {
    'uo-jsnum': { files: ['LICENSE-V8', 'LICENSE-optimized-routines'], dest: 'texts/uo-jsnum', component: 'V8 and ARM optimized-routines (math of uo-jsnum, translated to Rust)', licence: 'BSD-3-Clause AND MIT' },
    'uo-opc': { files: ['LICENSE-fflate'], dest: 'texts/uo-opc', component: 'fflate 0.8.3 (inflate of uo-opc, translated to Rust)', licence: 'MIT' },
    'uo-dml-render': { files: ['LICENSE-dingbat-to-unicode'], dest: 'texts/uo-dml-render', component: 'dingbat-to-unicode (symbol-font tables of uo-dml-render)', licence: 'BSD-2-Clause' },
  };
  for (const [art, crates] of Object.entries(artifacts)) {
    const entries = out[art];
    const names = new Set([...crates.values()].map((c) => c.name));
    for (const c of [...crates.values()].sort((a, b) => a.name.localeCompare(b.name))) {
      if (c.workspace) continue;
      if (!licenceOk(c.license, allow)) { bad.push(`${c.name} ${c.version}: ${c.license}`); continue; }
      const root = dirOf.get(`${c.name}@${c.version}`);
      if (!root) throw new Error(`gen-notices: ${c.name} ${c.version} not in cargo metadata`);
      const got = copyDir(root, `texts/${c.name}-${c.version}`, (f) => LICENSE_RE.test(f));
      if (!got.length) throw new Error(`gen-notices: ${c.name} ${c.version} has no licence file at its root`);
      entries.push({ component: c.name, version: c.version, licence: c.license, files: got });
    }
    for (const [name, w] of Object.entries(workspaceTexts)) {
      if (!names.has(name)) continue;
      const wc = [...crates.values()].find((c) => c.name === name);
      const root = dirOf.get(`${wc.name}@${wc.version}`);
      const got = w.files.map((f) => {
        const full = path.join(root, f);
        if (!existsSync(full)) throw new Error(`gen-notices: ${name}/${f} missing`);
        put(`${w.dest}/${f}`, Buffer.from(lf(readFileSync(full, 'latin1')), 'latin1'));
        return `${w.dest}/${f}`;
      });
      entries.push({ component: w.component, version: '', licence: w.licence, files: got });
    }
    if (names.has('uo-text')) {
      const src = path.join(stdDoc, 'licenses', 'Unicode-3.0.txt');
      put('texts/unicode/Unicode-3.0.txt', Buffer.from(lf(readFileSync(src, 'latin1')), 'latin1'));
      entries.push({ component: 'Unicode Character Database (tables of uo-text, generated through the ICU of Node 24.15)', version: '', licence: 'Unicode-3.0', files: ['texts/unicode/Unicode-3.0.txt'] });
    }
    const fonts = [...crates.values()].find((c) => c.name === 'uo-fonts');
    if (fonts?.features.has('hyphen-data')) {
      const hyph = path.join(dirOf.get(`uo-fonts@${fonts.version}`), 'data', 'hyphen', 'THIRD-PARTY-NOTICES.md');
      put('texts/hyphenation/THIRD-PARTY-NOTICES.md', Buffer.from(lf(readFileSync(hyph, 'latin1')), 'latin1'));
      entries.push({ component: 'Hyphenation patterns (hyph-utf8, CC0, Moby) compiled into uo-fonts', version: '', licence: 'see the notice file', files: ['texts/hyphenation/THIRD-PARTY-NOTICES.md'] });
    }
    entries.push({ component: 'Rust standard library (std, core, alloc and their dependencies)', version: '', licence: 'MIT OR Apache-2.0', files: stdFiles.map(([rel]) => rel) });
  }
  if (bad.length) throw new Error(`gen-notices: licence not in the allow list [${allow.join(', ')}]: ${bad.join('; ')}`);

  const lock = lockRegistry(path.join(CORE, 'Cargo.lock'));
  const json = JSON.stringify({ rustc, lock, artifacts: out }, null, 2) + '\n';
  put('notices.json', Buffer.from(json));
  put('THIRD-PARTY-NOTICES.md', Buffer.from(markdown(out)));
  return files;
}

function markdown(out) {
  const arts = Object.keys(out);
  const keys = new Map();
  for (const [art, list] of Object.entries(out)) {
    for (const c of list) {
      const k = `${c.component}|${c.version}`;
      const cur = keys.get(k) ?? { ...c, art: new Set() };
      cur.art.add(art);
      keys.set(k, cur);
    }
  }
  const lines = [
    '# Third-party notices of the Rust core',
    '',
    'The Rust core ships compiled in five artifacts. This file lists the third-party components in each, with the',
    'licence expression and the licence texts kept beside it (paths relative to this file).',
    '',
    '- wasm: `chunks/uo-write.wasm` in `@universal-office/write` and `@universal-office/document-viewer` (engine="rust").',
    '- wasm-editor: `chunks/uo-write-editor.wasm` in `@universal-office/write` (the editor, engine="rust").',
    '- wasm-slides: `chunks/uo-slides.wasm` in `@universal-office/slides-viewer` and `@universal-office/slides` (engine="rust").',
    '- wasm-slides-ppt: `chunks/uo-slides-ppt.wasm` in the same two packages (the lazily loaded .ppt converter).',
    '- native: `uo_ffi`, the native library inside the NuGet package `UniversalOffice.Docx`.',
    '',
    'Generated by `node core/tools/licenses/gen-notices.mjs`; do not edit by hand. The Universal Office code itself is MIT',
    '(see `LICENSE`).',
    '',
    '| Component | Version | Licence | ' + arts.join(' | ') + ' | Texts |',
    '| --- | --- | --- | ' + arts.map(() => '---').join(' | ') + ' | --- |',
  ];
  for (const c of [...keys.values()].sort((a, b) => a.component.localeCompare(b.component))) {
    lines.push(`| ${c.component} | ${c.version || '-'} | ${c.licence} | ${arts.map((a) => (c.art.has(a) ? 'yes' : 'no')).join(' | ')} | ${c.files.map((f) => `\`${f}\``).join('<br>')} |`);
  }
  return lines.join('\n') + '\n';
}

function walk(dir, base = dir) {
  const r = [];
  if (!existsSync(dir)) return r;
  for (const f of readdirSync(dir)) {
    const full = path.join(dir, f);
    if (statSync(full).isDirectory()) r.push(...walk(full, base));
    else r.push(path.relative(base, full).replace(/\\/g, '/'));
  }
  return r.sort();
}

function main() {
  const check = process.argv.includes('--check');
  const files = build();
  if (!check) {
    rmSync(OUT, { recursive: true, force: true });
    for (const [rel, buf] of files) {
      const dest = path.join(OUT, ...rel.split('/'));
      mkdirSync(path.dirname(dest), { recursive: true });
      writeFileSync(dest, buf);
    }
    console.log(`gen-notices: ${files.size} files in ${path.relative(process.cwd(), OUT) || '.'}`);
    return;
  }
  const problems = [];
  const onDisk = new Set(walk(OUT));
  const toolchain = (rel) => TOOLCHAIN_PREFIXES.some((p) => rel.startsWith(p));
  for (const [rel, buf] of files) {
    if (!onDisk.has(rel)) { problems.push(`missing: ${rel}`); continue; }
    if (toolchain(rel)) continue;
    let a = lf(readFileSync(path.join(OUT, rel), 'latin1'));
    let b = lf(buf.toString('latin1'));
    if (rel === 'notices.json') { // the toolchain version varies; everything else must match
      const strip = (t) => { const j = JSON.parse(t); delete j.rustc; for (const l of Object.values(j.artifacts)) for (const c of l) if (c.component.startsWith('Rust standard')) c.version = ''; return JSON.stringify(j); };
      a = strip(a); b = strip(b);
    }
    if (a !== b) problems.push(`differs: ${rel}`);
  }
  for (const rel of onDisk) if (!files.has(rel)) problems.push(`stale file: ${rel}`);
  if (problems.length) {
    console.error(`gen-notices: core/licenses is out of date:\n  ${problems.slice(0, 20).join('\n  ')}\nrun: node tools/licenses/gen-notices.mjs`);
    process.exit(1);
  }
  console.log(`gen-notices: core/licenses is current (${files.size} files)`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (e) { console.error(String(e.message ?? e)); process.exit(1); }
}
