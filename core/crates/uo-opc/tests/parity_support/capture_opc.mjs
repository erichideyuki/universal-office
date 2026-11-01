// Capture of the frozen TS container layer for the uo-opc parity (rust-port stage 4). Each app
// runs its OWN code (gate 1, G7): Write `DocxPackage` + docx-core `parseXml`, Sheet
// `XlsxPackage` + sheet-core `parseXml` (+ charts `canonicalNs` for chart and drawing parts),
// Slides `PptxPackage` + slides-core `parseXml`, the three CFB readers, and each package's
// `save()` read back by fflate. The Rust replay is tests/opc_parity.rs; the stream encodings
// below and in tests/parity_support/mod.rs must stay identical.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-opc/tests/parity_support/capture_opc.mjs
//     [--only <substring>] [--trace <key>]   (trace: prints the readable streams of one input)
//
// Inputs: every git-tracked .docx/.docm/.dotx/.xlsx/.xlsm/.pptx/.ppsx/.potx/.doc/.ppt/.xls of
// the checkout (the 2,742 oracle documents of docx-layout, sheet/samples, slides/samples and the
// other fixtures), the CFB parts embedded in them, and the generated quirk corpus
// (tests/parity_support/quirks.rs, written by `cargo run --example opc_quirk_corpus`).
// Output: tests/parity/opc.cap (format: core/crates/uo-testkit/src/capture.rs).
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import { CaptureWriter, Stream, importFrom, tsRoot, utf8Offsets } from '../../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const coreDir = resolve(here, '../../../..');
const out = join(here, '../parity/opc.cap');
const root = tsRoot();
const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : null;
const traceKey = argv.includes('--trace') ? argv[argv.indexOf('--trace') + 1] : null;

const { DocxPackage } = await importFrom(root, 'write/packages/docx-core/dist/package.js');
const docxXml = await importFrom(root, 'write/packages/docx-core/dist/xml.js');
const { XlsxPackage } = await importFrom(root, 'sheet/packages/sheet-core/dist/package.js');
const sheetXml = await importFrom(root, 'sheet/packages/sheet-core/dist/xml.js');
const charts = await importFrom(root, 'shared/charts/dist/xml.js');
const { PptxPackage } = await importFrom(root, 'slides/packages/slides-core/dist/package.js');
const slidesXml = await importFrom(root, 'slides/packages/slides-core/dist/xml.js');
const cfbReaders = {
  doc: (await importFrom(root, 'write/packages/docx-core/dist/doc/cfb.js')).readCfb,
  xls: (await importFrom(root, 'sheet/packages/sheet-core/dist/xls/cfb.js')).readCfb,
  ppt: (await importFrom(root, 'slides/packages/slides-core/dist/ppt/cfb.js')).readCfb,
};
const fflate = {
  w: await importFrom(root, 'write/node_modules/fflate/esm/index.mjs'),
  s: await importFrom(root, 'sheet/node_modules/fflate/esm/index.mjs'),
  p: await importFrom(root, 'slides/node_modules/fflate/esm/index.mjs'),
};
const tsCommit = execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();

// sheet-core loops forever on some unclosed constructs (gate 1, G5): parsed under a timeout
const vmCtx = vm.createContext({ sheetXml, charts, input: '' });
function guarded(expr, text) {
  vmCtx.input = text;
  try {
    return { tree: vm.runInContext(expr, vmCtx, { timeout: 5000 + Math.ceil(text.length / 100) }) };
  } catch (e) {
    return /timed out/.test(String(e?.message)) ? { hang: true } : { err: true };
  }
}
const plain = (f) => (text) => { try { return { tree: f(text) }; } catch { return { err: true }; } };

// preset id → parser (ids shared with tests/xml_support/mod.rs `preset`)
const parsers = {
  'docx-pos': plain((x) => docxXml.parseXml(x, { positions: true })),
  docx: plain((x) => docxXml.parseXml(x)),
  sheet: (x) => guarded('sheetXml.parseXml(input)', x),
  'sheet-chart': (x) => guarded('charts.canonicalNs(sheetXml.parseXml(input))', x),
  slides: plain((x) => slidesXml.parseXml(x)),
};

const asciiLower = (s) => s.replace(/[A-Z]/g, (c) => c.toLowerCase());
const XML_EXT = ['.xml', '.rels', '.vml', '.xaml', '.fdseq', '.fdoc', '.fpage'];
/** uo_opc::limits::is_xml_like */
const isXmlLike = (name) => {
  const base = asciiLower(name.slice(name.lastIndexOf('/') + 1));
  return base === '[content_types].xml' || XML_EXT.some((e) => base.endsWith(e));
};
const RELS_PART = /(^|\/)_rels\/[^/]*\.rels$/;
/** `dir/_rels/file.rels` → `dir/file`; `_rels/.rels` → `` */
const relsOwner = (p) => { const i = p.lastIndexOf('_rels/'); return p.slice(0, i) + p.slice(i + 6, -5); };

// The apps: each one's load, presets, relationship rule and lookup, as its code does today
const apps = {
  w: {
    load: (b) => DocxPackage.load(b),
    filter: undefined,
    presets: () => ['docx-pos'],
    relsScope: (p) => /^word\/_rels\/[^/]*\.rels$/.test(p),
    relsParser: 'docx',
    // document.ts buildReadContext: `./` stripped, absolute or `word/${target}`, exact lookup
    selected: (id, target, ext) => !!id && !!target && !ext,
    resolve: (pkg, owner, target) => { const t = target.replace(/^\.\//, ''); return t.startsWith('/') ? t.slice(1) : `word/${t}`; },
  },
  s: {
    load: (b) => XlsxPackage.load(b),
    filter: undefined,
    presets: (name) => (name.startsWith('xl/charts/') || name.startsWith('xl/drawings/') ? ['sheet', 'sheet-chart'] : ['sheet']),
    relsScope: () => true,
    relsParser: 'sheet',
    // drawing-charts.ts partRels / drawing.ts: id and target truthy, not external
    selected: (id, target, ext) => !!id && !!target && !ext,
    resolve: (pkg, owner, target) => pkg.resolveRelTarget(owner, target),
  },
  p: {
    load: (b) => PptxPackage.load(b),
    filter: (f) => f.originalSize <= Math.max(1 << 20, f.size * 1100),
    presets: () => ['slides'],
    relsScope: () => true,
    relsParser: 'slides',
    // read.ts readRels: id and target present, not external
    selected: (id, target, ext) => id !== undefined && target !== undefined && !ext,
    resolve: (pkg, owner, target) => pkg.resolveRelTarget(owner, target),
  },
};

function tree(st, el, off) {
  st.tag('E').str(el.name);
  const attrs = Object.entries(el.attrs);
  st.u32(attrs.length);
  for (const [k, v] of attrs) st.str(k).str(v);
  if (off) st.u64(off(el.s)).u64(off(el.cs)).u64(off(el.ce)).u64(off(el.e));
  if (el.raw !== undefined) st.tag('R').str(el.raw); else st.tag('r');
  st.u32(el.children.length);
  for (const c of el.children) {
    if (typeof c === 'string') st.tag('T').str(c);
    else tree(st, c, off);
  }
}

const traced = (key) => new Stream({ trace: key === traceKey });
const showTrace = (key, facet, st) => { if (key === traceKey) process.stdout.write(`--- ${key} ${facet}\n${st.trace()}`); };

function loadFacet(key, appId, bytes) {
  const app = apps[appId];
  let pkg;
  try {
    pkg = app.load(bytes);
  } catch {
    try {
      fflate[appId].unzipSync(bytes, app.filter ? { filter: app.filter } : undefined);
    } catch (e) {
      return { value: `err:zip:${e.message}` };
    }
    return { value: 'err:main' };
  }
  const st = traced(key);
  const names = pkg.partNames();
  for (const n of names) st.str(n).digestOf(pkg.getPart(n));
  showTrace(key, `load.${appId}`, st);
  return { pkg, value: `ok:${names.length}:${st.finish().short}` };
}

const slidesFails = (text) => { try { slidesXml.parseXml(text); return false; } catch { return true; } };

// G5 cases are listed in the facet g5.<app>: `hang:<preset>:<part>` (sheet-core never returns)
// and `garbage:<preset>:<part>:<tree digest>` (sheet-core returns a tree where slides-core, which
// fails on unclosed constructs, throws: the stage-3 rule). Both appear as one tag in the stream
// (`H`, `G`); the replay accepts uo-opc's unclosed-construct error there, or the same tree.
function xmlFacet(key, appId, pkg) {
  const app = apps[appId];
  const st = traced(key);
  let parsed = 0, errors = 0;
  const g5 = [];
  for (const name of pkg.partNames()) {
    if (!isXmlLike(name)) continue;
    const text = pkg.getPartText(name);
    for (const preset of app.presets(name)) {
      st.str(name).str(preset);
      const r = parsers[preset](text);
      if (r.tree && preset.startsWith('sheet') && slidesFails(text)) {
        parsed++;
        const own = new Stream();
        tree(own, r.tree, null);
        g5.push(`garbage:${preset}:${name}:${own.finish().short}`);
        st.tag('G');
      } else if (r.tree) {
        parsed++;
        tree(st, r.tree, preset === 'docx-pos' ? utf8Offsets(text) : null);
      } else if (r.hang) {
        errors++;
        g5.push(`hang:${preset}:${name}`);
        st.tag('H');
      } else {
        errors++;
        st.tag('X');
      }
    }
  }
  showTrace(key, `xml.${appId}`, st);
  return { value: `${parsed}:${errors}:${st.finish().short}`, g5 };
}

function relsFacet(key, appId, pkg, g5) {
  const app = apps[appId];
  const g5Parts = new Set(g5.map((g) => g.split(':').slice(2, g.startsWith('garbage:') ? -1 : undefined).join(':')));
  const st = traced(key);
  const names = new Set(pkg.partNames());
  let relsParts = 0, count = 0;
  for (const p of pkg.partNames()) {
    if (!RELS_PART.test(p) || !app.relsScope(p)) continue;
    relsParts++;
    st.str(p);
    const r = parsers[app.relsParser](pkg.getPartText(p));
    if (app.relsParser === 'sheet' && g5Parts.has(p)) { st.tag('G'); continue; }
    if (!r.tree) { st.tag('X'); continue; }
    const owner = relsOwner(p);
    for (const rel of r.tree.children) {
      if (typeof rel === 'string' || rel.name !== 'Relationship') continue;
      count++;
      const id = rel.attrs['Id'], target = rel.attrs['Target'], ext = rel.attrs['TargetMode'] === 'External';
      st.tag('L').bool(id !== undefined).str(id ?? '').bool(target !== undefined).str(target ?? '').bool(ext);
      if (!app.selected(id, target, ext)) { st.tag('S'); continue; }
      const resolved = app.resolve(pkg, owner, target);
      st.tag('R').str(resolved);
      if (pkg.getPart(resolved) !== undefined) st.tag('F').bool(names.has(resolved));
      else st.tag('N');
    }
  }
  showTrace(key, `rels.${appId}`, st);
  return `${relsParts}:${count}:${st.finish().short}`;
}

/** Central directory of an archive written by zipSync (names as bytes, flags, method). */
function centralDirectory(z) {
  const dv = new DataView(z.buffer, z.byteOffset, z.byteLength);
  let e = z.length - 22;
  while (dv.getUint32(e, true) !== 0x06054b50) e--;
  const count = dv.getUint16(e + 10, true);
  let at = dv.getUint32(e + 16, true);
  const list = [];
  for (let i = 0; i < count; i++) {
    const nl = dv.getUint16(at + 28, true), xl = dv.getUint16(at + 30, true), cl = dv.getUint16(at + 32, true);
    list.push({ name: z.subarray(at + 46, at + 46 + nl), flags: dv.getUint16(at + 8, true), method: dv.getUint16(at + 10, true) });
    at += 46 + nl + xl + cl;
  }
  return list;
}

const decodeName = (u8, flags) => (flags & 0x800 ? new TextDecoder().decode(u8) : String.fromCharCode(...u8));

function saveFacet(key, appId, pkg) {
  const saved = pkg.save();
  const back = fflate[appId].unzipSync(saved);
  const full = traced(key), noMethod = new Stream();
  const list = centralDirectory(saved);
  for (const c of list) {
    const data = back[decodeName(c.name, c.flags)];
    full.bytes(c.name).u32(c.flags & 0x800).u32(c.method).digestOf(data);
    noMethod.bytes(c.name).u32(c.flags & 0x800).digestOf(data);
  }
  showTrace(key, `save.${appId}`, full);
  return `${list.length}:${full.finish().short}:${noMethod.finish().short}`;
}

// An edit script on the app's own package (Map semantics: a set name keeps its position, a
// removed and set again name goes last, an integer-like name comes first in the zip), then saved.
function editFacet(key, appId, pkg) {
  const names = pkg.partNames();
  if (names.length < 3) return 'skip';
  const enc = new TextEncoder();
  pkg.setPart(names[1], enc.encode('edited'));
  if (appId === 's') pkg.deletePart(names[2]); else pkg.removePart(names[2]);
  pkg.setPart('new/part.xml', enc.encode('<new/>'));
  pkg.setPart(names[2], enc.encode('readded'));
  pkg.setPart('7', enc.encode('seven'));
  return saveFacet(key, appId, pkg);
}

const CFB_SIG = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const isCfb = (b) => b.length >= 8 && CFB_SIG.every((v, i) => b[i] === v);

function cfbFacets(cap, key, bytes) {
  for (const [copy, readCfb] of Object.entries(cfbReaders)) {
    let value;
    try {
      const f = readCfb(bytes);
      const st = traced(key);
      for (const e of f.entries) {
        st.str(e.name).str(e.path).str(e.type).str(BigInt(e.size).toString()).u32(e.start).bool(f.has(e.path));
        let d;
        try { d = f.getStream(e.path); } catch { st.tag('E'); continue; }
        if (d === undefined) st.tag('-'); else st.tag('D').digestOf(d);
      }
      showTrace(key, `cfb.${copy}`, st);
      value = `ok:${f.entries.length}:${st.finish().short}`;
    } catch {
      value = 'err';
    }
    cap.set(key, `cfb.${copy}`, value);
  }
}

const shortDigest = (u8) => new Stream().digestOf(u8).finish().short;

function packageFacets(cap, key, bytes, appIds) {
  const embedded = [];
  for (const appId of appIds) {
    const { pkg, value } = loadFacet(key, appId, bytes);
    cap.set(key, `load.${appId}`, value);
    if (!pkg) continue;
    const xml = xmlFacet(key, appId, pkg);
    cap.set(key, `xml.${appId}`, xml.value);
    if (xml.g5.length) cap.set(key, `g5.${appId}`, JSON.stringify(xml.g5));
    cap.set(key, `rels.${appId}`, relsFacet(key, appId, pkg, xml.g5));
    cap.set(key, `save.${appId}`, saveFacet(key, appId, pkg));
    cap.set(key, `edit.${appId}`, editFacet(key, appId, pkg));
    if (appId === appIds[0]) for (const n of pkg.partNames()) if (isCfb(pkg.getPart(n))) embedded.push([`${key}!${n}`, pkg.getPart(n)]);
  }
  for (const [k, b] of embedded) {
    cap.set(k, 'in', shortDigest(b));
    cfbFacets(cap, k, b);
  }
}

const APP_BY_EXT = { docx: 'w', docm: 'w', dotx: 'w', dotm: 'w', xlsx: 's', xlsm: 's', xltx: 's', pptx: 'p', pptm: 'p', ppsx: 'p', potx: 'p' };
const CFB_EXT = new Set(['doc', 'dot', 'xls', 'xlt', 'ppt', 'pps', 'pot']);

const cap = new CaptureWriter([
  'uo-opc parity capture (rust-port stage 4): the frozen TS container layer over the repository corpus and the quirk corpus.',
  'Written by core/crates/uo-opc/tests/parity_support/capture_opc.mjs; replayed by core/crates/uo-opc/tests/opc_parity.rs.',
  'Facets: in = input digest; load/xml/rels/save/g5 .w (Write) .s (Sheet) .p (Slides); cfb.doc/.xls/.ppt (the three CFB readers).',
]);
cap.meta('ts-commit', tsCommit);

const tracked = execFileSync('git', ['-C', root, 'ls-files', '-z'], { encoding: 'utf8' }).split('\0').filter(Boolean).sort();
const counts = { ooxml: 0, cfb: 0, quirks: 0 };
for (const rel of tracked) {
  const ext = rel.slice(rel.lastIndexOf('.') + 1).toLowerCase();
  const appId = APP_BY_EXT[ext];
  if (!appId && !CFB_EXT.has(ext)) continue;
  if (only && !rel.includes(only)) continue;
  const bytes = new Uint8Array(readFileSync(join(root, rel)));
  cap.set(rel, 'in', shortDigest(bytes));
  if (appId) { counts.ooxml++; packageFacets(cap, rel, bytes, [appId]); }
  else { counts.cfb++; cfbFacets(cap, rel, bytes); }
}

const qdir = mkdtempSync(join(tmpdir(), 'uo-opc-quirks-'));
try {
  execFileSync('cargo', ['run', '-q', '-p', 'uo-opc', '--example', 'opc_quirk_corpus', '--', qdir], { cwd: coreDir, stdio: ['ignore', 'ignore', 'inherit'] });
  for (const f of readdirSync(qdir).sort()) {
    const key = `quirk/${f.replace(/\.zip$/, '')}`;
    if (only && !key.includes(only)) continue;
    const bytes = new Uint8Array(readFileSync(join(qdir, f)));
    cap.set(key, 'in', shortDigest(bytes));
    counts.quirks++;
    packageFacets(cap, key, bytes, ['w', 's', 'p']);
  }
} finally {
  rmSync(qdir, { recursive: true, force: true });
}
cap.meta('inputs', `${counts.ooxml} ooxml, ${counts.cfb} cfb, ${counts.quirks} quirk archives`);
if (!only && !traceKey) cap.write(out);
console.log(`${counts.ooxml} OOXML packages, ${counts.cfb} CFB files, ${counts.quirks} quirk archives${only || traceKey ? ' (not written: --only/--trace)' : ` → ${out}`}`);
