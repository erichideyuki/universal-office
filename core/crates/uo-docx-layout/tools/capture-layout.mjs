// Layout parity capture of uo-docx-layout (record core/docs/modules/uo-docx-layout.md §5 and
// "Milestone 1 — F0"): runs the frozen TS engine (`layoutDocument`, built dist of main) over a
// corpus and writes, per document, the P1 canonical stream of the LayoutResult (the gate), the P2
// provider transcript (answers and their identity) and the P3 per-stage facets.
//
//   node tools/capture-layout.mjs --ts-root <built checkout of main> [mode] [options]
//
// Modes:
//   --mode ci --count N --seed S    N generated documents with open fonts only (CI layout parity,
//                                   LQ21); --docs-out <dir> keeps the .docx for the Rust replay
//   --mode oracle [--only g/c,…] [--max N]
//                                   the Word oracle (write/packages/docx-layout/test/oracle) with the
//                                   machine's installed fonts (never versioned). LQ21: for the gate
//                                   the capture is regenerated in the same run as the Rust replay,
//                                   on the same machine, and a face whose digest differs fails it
//   --mode files <a.docx> …         the given files, installed fonts (the .NET synthetic inputs:
//                                   dotnet/tests/fixtures/input/*.docx)
// Options:
//   --out <file>      capture file (default tests/captures/layout-<mode>.capture)
//   --dump <dir>      per document: <key>.p1.bin (the raw P1 stream, for the Rust adapter check and
//                     for locating a mismatch), <key>.transcript.tsv (P2 answers with digests and
//                     sources, what the Rust replay loads its faces from), <key>.media/<sha256>.bin
//                     (the image payloads, for the adapter check)
//   --trace           also writes <key>.p1.trace (one item per line) into the dump dir
//
// Determinism (record §5): TZ=UTC (set below, before any Date), a fixed `now`, every document read
// again for every run (K3: the TS frame caches are keyed by model objects), the 7 hyphenation
// pattern sets loaded as in oracle.test.ts (the production host list is a separate sweep, written
// in the record), `mk-*` oracle cases with `revisions: { mode: 'all' }`, one process per capture
// (lineSeq is process-global; the walker relabels it anyway).
process.env.TZ = 'UTC';

const { mkdirSync, readFileSync, writeFileSync, existsSync } = await import('node:fs');
const { join, dirname, basename } = await import('node:path');
const { fileURLToPath } = await import('node:url');
const { execFileSync } = await import('node:child_process');
const { CaptureWriter, importFrom, tsRoot, walk, relPath, Rng } = await import('../../uo-testkit/js/testkit.mjs');
const { installFacetHooks, facetRecorder } = await import('./lib/facets.mjs');
const { canonLayout, CANON_VERSION } = await import('./lib/canon.mjs');
const { Sink } = await import('./lib/sink.mjs');
const { recordProvider } = await import('./lib/transcript.mjs');
const { generateDocx } = await import('./lib/gen-docx.mjs');
const { generateF7aDocx } = await import('./lib/gen-f7a.mjs');
const { generateF7cDocx } = await import('./lib/gen-f7c.mjs');
const { generateF7bDocx } = await import('./lib/gen-f7b.mjs');
// `--f8`: the P4 facets of F8 (tools/lib/facets-f8.mjs: editor/viewer queries, onField, forced
// hyphenation, the Web view) — added by F8
const f8 = process.argv.includes('--f8') ? await import('./lib/facets-f8.mjs') : null;
const { generateF7dDocx } = await import('./lib/gen-f7d.mjs');
// `--g8`: the Write page section as the viewer gets it, and the viewer's annotations
// (tools/lib/facets-g8.mjs; record "Write page section (G8)") — added by G8
const g8 = process.argv.includes('--g8') ? await import('./lib/facets-g8.mjs') : null;

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name, d) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : d; };
const flag = (name) => argv.includes(name);
const root = tsRoot(argv);
const mode = opt('--mode', 'ci');

// F2-20 (added by F7a): refuse a stale TS build — a source module without its dist module, or a
// dist module older than its source. `--accept-stale a.ts,b.ts` names sources whose newer edit is
// known not to change results (the frozen d0f45bc5 build: typography/font/sfnt.ts, a GPOS cache).
{
  const { statSync, readdirSync } = await import('node:fs');
  const { relative } = await import('node:path');
  const accept = new Set((opt('--accept-stale', '') || '').split(',').filter(Boolean));
  const files = (d, o = []) => { for (const n of readdirSync(d)) { const p = join(d, n); if (statSync(p).isDirectory()) files(p, o); else o.push(p); } return o; };
  const stale = [];
  for (const pkg of ['write/packages/docx-layout', 'write/packages/docx-core']) {
    const src = join(root, pkg, 'src');
    for (const ts of files(src).filter((p) => p.endsWith('.ts') && !p.endsWith('.d.ts'))) {
      const rel = relative(src, ts).split('\\').join('/');
      const js = join(root, pkg, 'dist', rel.replace(/\.ts$/, '.js'));
      if (!existsSync(js)) stale.push(`${pkg}/dist/${rel.replace(/\.ts$/, '.js')} (missing)`);
      else if (statSync(js).mtimeMs < statSync(ts).mtimeMs && !accept.has(rel)) stale.push(`${pkg}/src/${rel} (newer than its dist)`);
    }
  }
  if (stale.length) { console.error(`stale TS build at ${root} (rebuild it, or --accept-stale):\n  ${stale.join('\n  ')}`); process.exit(2); }
}

// the loader hook of the P3 facets must be in place before the engine is imported
installFacetHooks(root);
const core = await importFrom(root, 'write/packages/docx-core/dist/index.js');
const eng = await importFrom(root, 'write/packages/docx-layout/dist/index.js');
const { LineBreaker } = await importFrom(root, 'write/packages/docx-layout/dist/lines.js');
const fflate = await importFrom(root, 'write/node_modules/fflate/esm/index.mjs');

/** DATE/TIME and friends (LQ10): fixed instant, UTC. */
export const NOW = new Date(Date.UTC(2026, 0, 15, 12, 0, 0));

if (mode === 'ci') {
  // open fonts only: node-fonts indexes WINDIR/Fonts, LOCALAPPDATA and EXTRA_FONT_DIRS — point the
  // first two at nothing and the last at the repository's open fonts
  process.env.WINDIR = join(here, '__no_system_fonts__');
  process.env.LOCALAPPDATA = join(here, '__no_system_fonts__');
  process.env.EXTRA_FONT_DIRS = [join(root, 'shared/fonts/files'), join(root, 'site/assets/fonts')].join(';');
}
const { createNodeFontProvider } = await importFrom(root, 'write/tools/corpus-lab/node-fonts.mjs');
const provider = await createNodeFontProvider();
await eng.loadHyphenation(eng.HYPHENATION_PATTERN_IDS);

if (mode !== 'ci') {
  const missing = ['Arial', 'Calibri', 'Times New Roman'].filter((f) => !provider.face(f, false, false));
  if (missing.length) { console.error(`missing installed fonts (${missing.join(', ')}): the oracle needs them; nothing written`); process.exit(2); }
}

/** The documents: { key, bytes, opts }. */
function corpus() {
  if (mode === 'ci') {
    const count = Number(opt('--count', '20')), seed = Number(opt('--seed', '1'));
    // `--gen f7a`: the engine-core generator (tools/lib/gen-f7a.mjs, F7a): no header/footer,
    // table, float or note — what F7a lays out end to end
    const f7a = opt('--gen', '') === 'f7a';
    // `--gen f7c`: headers/footers, notes, endnotes and the SEQ/REF/… fields (tools/lib/gen-f7c.mjs, F7c)
    const f7c = opt('--gen', '') === 'f7c';
    // `--gen f7b`: tables, floating tables, floating shapes/textboxes and frames (tools/lib/gen-f7b.mjs, F7b)
    const f7b = opt('--gen', '') === 'f7b';
    // `--gen f7d`: linked textbox chains and endnote-flow tables (tools/lib/gen-f7d.mjs, F7d)
    const f7d = opt('--gen', '') === 'f7d';
    // `--gen f8`: an F7c or F7b document (by index parity) with links and plugin inlines
    // (tools/lib/facets-f8.mjs, F8)
    const gf8 = opt('--gen', '') === 'f8';
    const out = [];
    for (let i = 0; i < count; i++) {
      const s = (seed * 1000003 + i) >>> 0;
      out.push(gf8
        ? { key: `f8/${seed}/${i}`, bytes: f8.decorateF8(i % 2 ? generateF7bDocx(s, fflate) : generateF7cDocx(s, fflate), s, fflate, Rng), opts: {} }
        : f7d
        ? { key: `f7d/${seed}/${i}`, bytes: generateF7dDocx(s, fflate), opts: {} }
        : f7b
        ? { key: `f7b/${seed}/${i}`, bytes: generateF7bDocx(s, fflate), opts: {} }
        : f7c
        ? { key: `f7c/${seed}/${i}`, bytes: generateF7cDocx(s, fflate), opts: {} }
        : f7a
        ? { key: `f7a/${seed}/${i}`, bytes: generateF7aDocx(s, fflate), opts: {} }
        : { key: `gen/${seed}/${i}`, bytes: generateDocx(s, fflate), opts: {} });
    }
    return out;
  }
  if (mode === 'oracle') {
    const dir = join(root, 'write/packages/docx-layout/test/oracle');
    const only = opt('--only') ? new Set(opt('--only').split(',').map((s) => s.trim().replace(/\.docx$/i, ''))) : null;
    const max = Number(opt('--max', '1e9'));
    return walk(dir, (p) => /\.docx$/i.test(p))
      .map((p) => ({ key: relPath(dir, p).replace(/\.docx$/i, ''), path: p }))
      .filter((c) => !only || only.has(c.key))
      .slice(0, max)
      .map((c) => ({ key: c.key, bytes: new Uint8Array(readFileSync(c.path)), opts: basename(c.key).startsWith('mk-') ? { revisions: { mode: 'all' } } : {} }));
  }
  const VALUED = new Set(['--ts-root', '--mode', '--out', '--dump', '--docs-out', '--count', '--seed', '--only', '--max', '--accept-stale', '--gen']);
  const files = argv.filter((a, i) => !a.startsWith('--') && !VALUED.has(argv[i - 1]) && a !== 'files');
  return files.map((p) => ({ key: basename(p).replace(/\.docx$/i, ''), bytes: new Uint8Array(readFileSync(p)), opts: {} }));
}

const out = opt('--out', join(here, '..', 'tests', 'captures', `layout-${mode}.capture`));
const dump = opt('--dump');
const docsOut = opt('--docs-out');
for (const d of [dump, docsOut, dirname(out)]) if (d) mkdirSync(d, { recursive: true });
const safe = (key) => key.replace(/[\\/:]/g, '__');

let commit = '';
try { commit = execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(); } catch { /* not a git checkout */ }
const cap = new CaptureWriter([
  'uo-docx-layout layout parity capture: written by core/crates/uo-docx-layout/tools/capture-layout.mjs, replayed by tests/layout_parity.rs (record §5, "Milestone 1 — F0").',
  'p1: the canonical stream of the LayoutResult (tools/lib/canon.mjs = src/canon.rs); p2/p2first: the provider transcript (tools/lib/transcript.mjs); p3.*: per-stage facets (tools/lib/facets.mjs); error: the TS exception, if the layout threw.',
]);
cap.meta('ts-commit', commit || 'unknown');
cap.meta('mode', mode);
cap.meta('canon-version', CANON_VERSION);
cap.meta('now', NOW.toISOString());
cap.meta('tz', process.env.TZ);
cap.meta('hyphenation-loaded', eng.HYPHENATION_PATTERN_IDS.join(','));

const F8 = f8 ? await f8.setupF8(root) : null;
const G8 = g8 ? await g8.setupG8(root, importFrom) : null;
const facets = facetRecorder({
  LineBreaker, FaceResolver: eng.FaceResolver, SfntFont: eng.SfntFont,
  // F5 (p3.lines, p3.paint): the helpers its recorder calls with the reporter off
  f5: { advanceTwips: (await importFrom(root, 'write/packages/docx-layout/dist/para.js')).advanceTwips, faceMetrics: eng.faceMetrics, fontOfSlot: eng.fontOfSlot },
  // F2 (p3.datefmt, p3.formula, p3.numbering, p3.style, p3.hyphlangs): the classes it patches
  f2: { NumberingState: eng.NumberingState, StyleResolver: eng.StyleResolver, documentHyphenationLanguages: eng.documentHyphenationLanguages },
  // F6 (p3.table, p3.rowsplit, p3.splitcontent): the z-order marks of cell floats
  f6: { LATE_ITEMS: (await importFrom(root, 'write/packages/docx-layout/dist/table.js')).LATE_ITEMS },
});
let pages = 0, errors = 0;
const docs = corpus();
for (const doc of docs) {
  let bytes = doc.bytes;
  if (docsOut) writeFileSync(join(docsOut, `${safe(doc.key)}.docx`), bytes);
  try {
    if (core.isWordBinary(bytes)) bytes = core.convertDocToDocx(bytes);
    // read again for every run (K3)
    const pkg = core.DocxPackage.load(bytes);
    const full = core.readFullDocument(pkg);
    let embedded = [];
    try { embedded = core.readEmbeddedFonts(pkg); } catch { /* unreadable embedded fonts: none */ }
    const rec = recordProvider(eng.withEmbeddedFonts(provider, embedded, (b) => new eng.SfntFont(b)));
    facets.start();
    let res;
    const fields = new Map();
    const onField = f8 ? { onField: (r, v) => { fields.set(r, v); } } : {};
    try { res = eng.layoutDocument(full, rec.provider, { now: NOW, ...doc.opts, ...onField }); } finally { Object.assign(doc, { f3: facets.finish() }); }
    const sink = new Sink({ raw: !!dump, trace: flag('--trace') });
    const media = dump ? new Map() : null;
    canonLayout(res, full, sink, media);
    const p1 = sink.finish();
    cap.set(doc.key, 'p1', p1.value);
    const p2 = rec.facets();
    cap.set(doc.key, 'p2', p2.p2);
    cap.set(doc.key, 'p2first', p2.p2first);
    for (const [k, v] of Object.entries(doc.f3)) cap.set(doc.key, k, v);
    cap.set(doc.key, 'pages', String(res.pages.length));
    if (g8) for (const [k, v] of Object.entries(g8.g8Facets(G8, full, res))) cap.set(doc.key, k, v);
    if (f8) {
      // after the P1 stream and the transcript facets: the extra layouts only append calls
      for (const [k, v] of Object.entries(f8.editorFacets(F8, full, res, fields, safe(doc.key)))) cap.set(doc.key, k, v);
      const read = () => core.readFullDocument(core.DocxPackage.load(bytes));
      const layout = (fd, o) => eng.layoutDocument(fd, rec.provider, { now: NOW, ...doc.opts, ...o });
      for (const [k, v] of Object.entries(f8.layoutFacets(F8, read, layout))) cap.set(doc.key, k, v);
    }
    pages += res.pages.length;
    if (dump) {
      writeFileSync(join(dump, `${safe(doc.key)}.p1.bin`), p1.bytes);
      writeFileSync(join(dump, `${safe(doc.key)}.transcript.tsv`), rec.transcriptText());
      for (const [name, text] of Object.entries(doc.f3.dumps ?? {})) writeFileSync(join(dump, `${safe(doc.key)}.${name}`), text);
      if (media.size) {
        const md = join(dump, `${safe(doc.key)}.media`);
        mkdirSync(md, { recursive: true });
        for (const [h, u8] of media) writeFileSync(join(md, `${h}.bin`), u8);
      }
      if (flag('--trace')) writeFileSync(join(dump, `${safe(doc.key)}.p1.trace`), p1.trace);
    }
  } catch (e) {
    errors++;
    cap.set(doc.key, 'error', String(e?.message ?? e).replace(/[\t\n\r]/g, ' ').slice(0, 300));
  }
}
cap.meta('documents', docs.length);
cap.meta('pages', pages);
cap.meta('errors', errors);
cap.write(out);
console.log(`${docs.length} documents, ${pages} pages, ${errors} errors -> ${out}${dump ? ` (dump ${dump})` : ''}`);
if (!existsSync(out)) process.exit(1);
