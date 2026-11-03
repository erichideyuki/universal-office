// F1 resolver sweep (record "Wave 1 — F1"): the frozen TS `FaceResolver` (fonts.ts:224, the Word
// substitution chain) over seeded random font tables and requests, because the layout corpus
// exercises little of the chain (the oracle documents ask ~1 distinct family each and node-fonts
// never answers a stand-in). Writes, per scenario, the facets `p3.faces` and `p2first` (the same
// streams as tools/lib/facets-f1.mjs and tools/lib/transcript.mjs) and one combined dump of the
// inputs, replayed by tests/f1_parity.rs.
//
//   node tools/capture-f1-resolver.mjs --ts-root <built main> [--count N] [--seed S] --out <file> --dump <dir>
//
// Fonts: the repository's open fonts only (node-fonts roots pointed as in capture-layout --mode ci),
// behind a stand-in wrapper that answers some absent families with a metric substitute marked
// `standIn` (one object per request key, as the viewer's per-call cache), so the stand-in rule of
// the resolver runs. The dump `<dir>/resolver.f1.tsv` and `<dir>/resolver.transcript.tsv` hold one
// block per scenario, each opened by `S <key>`; the block formats are those of `<key>.f1.tsv` and
// `<key>.transcript.tsv`.
process.env.TZ = 'UTC';

const { mkdirSync, writeFileSync } = await import('node:fs');
const { join, dirname } = await import('node:path');
const { fileURLToPath } = await import('node:url');
const { execFileSync } = await import('node:child_process');
const { CaptureWriter, importFrom, tsRoot } = await import('../../uo-testkit/js/testkit.mjs');
const { f1Recorder } = await import('./lib/facets-f1.mjs');
const { recordProvider } = await import('./lib/transcript.mjs');

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name, d) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : d; };
const root = tsRoot(argv);
const count = Number(opt('--count', '200')), seed = Number(opt('--seed', '1'));
const out = opt('--out', join(here, '..', 'tests', 'captures', 'f1', 'resolver.capture'));
const dump = opt('--dump', join(here, '..', 'tests', 'captures', 'f1'));

process.env.WINDIR = join(here, '__no_system_fonts__');
process.env.LOCALAPPDATA = join(here, '__no_system_fonts__');
process.env.EXTRA_FONT_DIRS = [join(root, 'shared/fonts/files'), join(root, 'site/assets/fonts')].join(';');
const eng = await importFrom(root, 'write/packages/docx-layout/dist/index.js');
const { createNodeFontProvider } = await importFrom(root, 'write/tools/corpus-lab/node-fonts.mjs');
const node = await createNodeFontProvider();

/** mulberry32 */
function rng(s) {
  let a = s >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const FAMILIES = [
  'Arial', 'arial', ' ARIAL ', 'Helvetica', 'Helvetica Neue', 'ArialMT', 'Arial MT', 'Times', 'Times New Roman', 'TimesNewRomanPSMT',
  'Courier', 'Courier New', 'CourierNewPSMT', 'Arial-BoldMT', 'Arial-ItalicMT', 'Calibri', 'Calibri-Italic', 'Calibri Light', 'Cambria',
  'Liberation Sans', 'Liberation Serif', 'Liberation Mono', 'Carlito', 'Caladea', 'MS Mincho', 'MS Gothic', 'Arial Unicode MS', 'Yu Gothic',
  'SimSun', 'Batang', 'PMingLiU', 'Inter', 'Mystery', 'Symbol', 'SymbolMT', 'Wingdings', 'Wingdings-Regular', 'ZapfDingbats',
  'Verdana,BoldOblique', 'LiberationSerif-Bold', 'Foo-Bar', 'Ms Shell Dlg 2', 'Segoe UI', 'Sakkal Majalla', '', '  ', '　Carlito',
];
const STAND_IN = { 'arial unicode ms': 'Liberation Sans', 'ms mincho': 'Liberation Serif', mystery: 'Carlito', calibri: 'Carlito', cambria: 'Caladea', 'segoe ui': 'Liberation Sans' };
const CHARSETS = ['80', '81', '86', '88', '02', '00', 'ee', undefined];
const GENERIC = ['roman', 'swiss', 'modern', 'script', 'decorative', 'auto', undefined];
const SERIF = ['02', '0B', '0C', '0D', '05', '00', 'zz'];
const USB0 = ['00002000', 'E0002AFF', '00000003', '', 'nothex', undefined];

/** The host provider with stand-ins: an absent family of STAND_IN gets its substitute's face,
 *  marked standIn, one object per request (the viewer's per-call cache). */
function withStandIns(provider) {
  const cache = new Map();
  return {
    face(family, bold, italic) {
      const sub = STAND_IN[family.toLowerCase()];
      if (!sub) return provider.face(family, bold, italic);
      const key = `${family}|${bold ? 1 : 0}${italic ? 1 : 0}`;
      if (cache.has(key)) return cache.get(key);
      const base = provider.face(sub, bold, italic);
      const f = base ? { ...base, key: `${family}|standin|${base.key}`, family, standIn: true } : null;
      cache.set(key, f);
      return f;
    },
  };
}

function scenario(r) {
  const pick = (a) => a[Math.floor(r() * a.length)];
  const fontTable = {};
  const n = Math.floor(r() * 5);
  for (let i = 0; i < n; i++) {
    const name = pick(FAMILIES);
    const info = {};
    const cs = pick(CHARSETS); if (cs !== undefined) info.charset = r() < 0.2 ? cs.toLowerCase() : cs;
    const g = pick(GENERIC); if (g !== undefined) info.family = g;
    if (r() < 0.4) info.altName = Array.from({ length: 1 + Math.floor(r() * 2) }, () => ` ${pick(FAMILIES)} `).join(r() < 0.5 ? ',' : ';');
    if (r() < 0.4) info.panose1 = `02${pick(SERIF)}0604020202020204`.slice(0, r() < 0.1 ? 3 : 20);
    const u = pick(USB0); if (u !== undefined && r() < 0.5) info.sig = { usb0: u, usb1: '0' };
    fontTable[name] = info;
  }
  const gets = Array.from({ length: 1 + Math.floor(r() * 12) }, () => [r() < 0.05 ? undefined : pick(FAMILIES), r() < 0.3, r() < 0.3]);
  return { fontTable, gets };
}

let commit = '';
try { commit = execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(); } catch { /* not a git checkout */ }
const cap = new CaptureWriter([
  'uo-docx-layout F1 resolver sweep: written by core/crates/uo-docx-layout/tools/capture-f1-resolver.mjs, replayed by tests/f1_parity.rs (record "Wave 1 — F1").',
  'p3.faces: FaceResolver.get per distinct request (tools/lib/facets-f1.mjs); p2first: the first provider request per lower-case key (tools/lib/transcript.mjs).',
]);
cap.meta('ts-commit', commit || 'unknown');
cap.meta('seed', seed);
cap.meta('count', count);
const f1 = f1Recorder({ FaceResolver: eng.FaceResolver, SfntFont: eng.SfntFont });
const dumpF1 = [], dumpT = [];
let errors = 0;
for (let i = 0; i < count; i++) {
  const key = `resolver/${seed}/${i}`;
  const { fontTable, gets } = scenario(rng((seed * 1000003 + i) >>> 0));
  const rec = recordProvider(withStandIns(node));
  const res = new eng.FaceResolver(rec.provider, fontTable);
  f1.start();
  try {
    for (const [family, b, it] of gets) {
      try { res.get(family, b, it); } catch { errors++; }
    }
  } finally {
    const r = f1.finish();
    cap.set(key, 'p3.faces', r.facets['p3.faces']);
    cap.set(key, 'p2first', rec.facets().p2first);
    dumpF1.push(`S\t${key}`, r.dump.trimEnd());
    dumpT.push(`S\t${key}`, rec.transcriptText().trimEnd());
  }
}
cap.meta('throws', errors);
mkdirSync(dump, { recursive: true });
mkdirSync(dirname(out), { recursive: true });
writeFileSync(join(dump, 'resolver.f1.tsv'), dumpF1.join('\n') + '\n');
writeFileSync(join(dump, 'resolver.transcript.tsv'), dumpT.join('\n') + '\n');
cap.write(out);
console.log(`${count} scenarios (${errors} throws) -> ${out} (dump ${dump})`);
