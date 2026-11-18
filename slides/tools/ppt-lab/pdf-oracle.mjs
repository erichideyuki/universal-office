// PowerPoint-PDF oracle of the Rust Slides PDF (M4 gate prep; record core/docs/modules/uo-pdf.md "PowerPoint agreement").
//
//   node slides/tools/ppt-lab/pdf-oracle.mjs list            # writes core/crates/uo-pptx/tests/oracle_ppt/decks.tsv
//   node slides/tools/ppt-lab/pdf-oracle.mjs ppt [--force]   # PowerPoint PDFs of every deck (COM, no window, cached by deck sha)
//   node slides/tools/ppt-lab/pdf-oracle.mjs rust            # the Rust PDFs (cargo test --test oracle_ppt, fonts system + open)
//   node slides/tools/ppt-lab/pdf-oracle.mjs ts              # the TS viewer's canvas of every slide (Chromium, 96 dpi PNG)
//   node slides/tools/ppt-lab/pdf-oracle.mjs compare         # pdfium comparison, cause classes, offsets by font -> tests/oracle_ppt/results/*
//   node slides/tools/ppt-lab/pdf-oracle.mjs all
//
// Work files (PDFs, PNGs) live outside git in $UO_PPT_ORACLE_OUT (default core/target/w4-oracle/ppt-oracle); only digests and
// metrics are committed. PowerPoint runs through powershell with windowsHide, opens each deck read-only WITHOUT a window
// (export-pdf.ps1) and never touches a presentation it did not open (the no-focus-steal rule).
import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../..');
const core = join(repo, 'core');
const oracleDir = join(core, 'crates/uo-pptx/tests/oracle_ppt');
const out = resolve(process.env.UO_PPT_ORACLE_OUT ?? join(core, 'target/w4-oracle/ppt-oracle'));
const fwd = (p) => p.replace(/\\/g, '/');
const sha = (b) => createHash('sha256').update(b).digest('hex');

/** The generated groups: every `step`-th deck from the first (all of them where PowerPoint refuses most decks as corrupt; b3a decks
 * take ~15 s each in PowerPoint because of their CJK/Arabic fallback fonts, so every 5th). */
const GENERATED = [['b1a', 1], ['b3a', 5], ['b5a', 1], ['b5b', 1], ['b6a', 1], ['c1d', 1]];

function readDecks() {
  const rows = readFileSync(join(oracleDir, 'decks.tsv'), 'utf8').split('\n').filter((l) => l && !l.startsWith('#'));
  return rows.map((l) => { const [id, path, group, digest, features] = l.split('\t'); return { id, path, group, digest, features: features ? features.split(',') : [] }; });
}

async function list() {
  const rows = [];
  const tracked = execFileSync('git', ['-C', repo, 'ls-files', 'slides/samples'], { encoding: 'utf8' }).split('\n').map((s) => s.trim()).filter((s) => /\.(pptx|ppt)$/i.test(s)).sort();
  for (const p of tracked) {
    const id = p.replace(/^slides\/samples\//, 's_').replace(/[/]/g, '_');
    const group = p.includes('/legacy/') ? 'legacy' : p.includes('/playback/') ? (p.endsWith('.ppt') ? 'playback-ppt' : 'playback') : 'samples';
    rows.push([id, p, group, sha(readFileSync(join(repo, p))).slice(0, 16), '']);
  }
  const { buildGroupDeck } = await import(pathToFileURL(join(core, 'crates/uo-pptx/js/lib/gen/groups.mjs')).href);
  for (const [g, step] of GENERATED) {
    const dir = join(core, 'crates/uo-pptx/tests/captures/decks', g);
    const names = readdirSync(dir).filter((f) => f.endsWith('.pptx')).sort();
    for (let i = 0; i < names.length; i += step) {
      const p = fwd(join('core/crates/uo-pptx/tests/captures/decks', g, names[i]));
      const bytes = readFileSync(join(repo, p));
      const d = buildGroupDeck(g, Number(names[i].replace(/\.pptx$/, '')));
      // features of the current generator (the committed bytes may differ by the zlib of the Node that wrote them)
      rows.push([`${g}_${names[i].replace(/\.pptx$/, '')}`, p, g, sha(bytes).slice(0, 16), [...d.features].sort().join(',')]);
    }
  }
  const head = '# id\tpath (repo-relative)\tgroup\tsha256 of the deck (16 hex)\tgenerator features (generated decks)\n';
  writeFileSync(join(oracleDir, 'decks.tsv'), head + rows.map((r) => r.join('\t')).join('\n') + '\n');
  console.log(`decks.tsv: ${rows.length} decks`);
}

function ppt(force) {
  const onlyAt = process.argv.indexOf('--only');
  const only = onlyAt > 0 ? new Set(process.argv[onlyAt + 1].split(',')) : null;
  const decks = readDecks().filter((d) => !only || only.has(d.id));
  const dir = join(out, 'ppt');
  mkdirSync(dir, { recursive: true });
  const todo = decks.filter((d) => {
    if (force) return true;
    // cached: a PDF or a recorded refusal (.info failed=) of the same deck bytes
    const stamp = join(dir, `${d.id}.deck`);
    return !(existsSync(join(dir, `${d.id}.info`)) && existsSync(stamp) && readFileSync(stamp, 'utf8').trim() === d.digest);
  });
  console.log(`ppt: ${decks.length - todo.length} cached, ${todo.length} to export`);
  // batches: one powershell (one PowerPoint session) per 20 decks, so a stuck deck costs one batch
  for (let i = 0; i < todo.length; i += 20) {
    const batch = todo.slice(i, i + 20);
    const listFile = join(dir, 'batch.tsv');
    writeFileSync(listFile, '﻿' + batch.map((d) => `${d.id}\t${resolve(repo, d.path)}`).join('\r\n') + '\r\n');
    const r = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', join(here, 'export-pdf.ps1'), '-List', listFile, '-OutDir', dir],
      { windowsHide: true, encoding: 'utf8', timeout: 20 * 60 * 1000 });
    process.stdout.write(r.stdout ?? '');
    if (r.status !== 0) console.log(`batch ${i / 20}: exit ${r.status} ${r.error ?? ''} ${(r.stderr ?? '').slice(0, 2000)}`);
    for (const d of batch) if (existsSync(join(dir, `${d.id}.pdf`))) writeFileSync(join(dir, `${d.id}.deck`), d.digest + '\n');
  }
}

function rust() {
  const env = { ...process.env, UO_PPT_ORACLE_OUT: out, CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR ?? join(core, 'target/w4-oracle') };
  const r = spawnSync('cargo', ['test', '-p', 'uo-pptx', '--release', '--features', 'ppt', '--test', 'oracle_ppt', '--', '--nocapture'], { cwd: core, env, stdio: 'inherit', windowsHide: true });
  if (r.status !== 0) process.exit(r.status ?? 1);
}

function ts() {
  const r = spawnSync(process.execPath, [join(here, 'pdf-oracle-ts.mjs')], { env: { ...process.env, UO_PPT_ORACLE_OUT: out }, stdio: 'inherit', windowsHide: true });
  if (r.status !== 0) process.exit(r.status ?? 1);
}

function compare() {
  // the comparison, then the cause classes and the glyph offsets by font (both read the comparison's results)
  for (const script of ['pdf-oracle-compare.py', 'pdf-oracle-classify.py', 'pdf-oracle-baseline.py']) {
    const r = spawnSync('python', ['-I', join(here, script), out, oracleDir], { stdio: 'inherit', windowsHide: true });
    if (r.status !== 0) process.exit(r.status ?? 1);
  }
}

const cmd = process.argv[2];
if (cmd === 'list') await list();
else if (cmd === 'ppt') ppt(process.argv.includes('--force'));
else if (cmd === 'rust') rust();
else if (cmd === 'ts') ts();
else if (cmd === 'compare') compare();
else if (cmd === 'all') { ppt(false); rust(); ts(); compare(); }
else { console.error('usage: pdf-oracle.mjs list|ppt [--force]|rust|ts|compare|all'); process.exit(2); }
