// Parity capture of the D1b frontier W4/W4b: the frozen TS `writeDefaultTabStop`, `writeHyphenationSettings`,
// `writeEvenAndOddHeaders`, `writeDisplayBackgroundShape` (settings-xml.ts), `ensureNoteParts`,
// `patchSettingsNotes` (notes.ts), `hfPartXml`, `copyHfPartXml`, `dropUnusedHfParts`, `pageNumberGroupIds`
// (hf-ops.ts) over the settings, relationships, content types, notes and header/footer parts of the corpus
// (the oracle documents of docx-layout and the docx-core fixtures), the 314 distinct mini packages (their
// parts are written to tests/parity/settings-xml.inputs), each as it is and in named variants of the
// settings/relationships/content types (missing, self-closing, bare, stripped of the elements, flags off,
// prefixed namespace, BOM, truncated, ...), with seeded-by-list arguments.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-settings-xml.mjs
//   UO_EXPLAIN='<key>' node ...   prints the summary text of one record (the facet is its digest)
//
// tests/parity/settings-xml.capture: one record per `<docx>#<variant>#<op>` (a package op: facet `out` = short
// SHA-256 of the summary: the part order and each changed part's short SHA-256, or `err` when the TS throws;
// a pure op: of the output text), `args` where the arguments came from the package (the ids to drop), and per
// `pn#<docx>` the positions of the page-number blocks of each story (`pageNumberGroupIds`).
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { corpus, crate, readBytes, ts } from './capture-r2-common.mjs';
import { quirkSources, QUIRK_SEED, QUIRK_COUNT } from './quirk-sources.mjs';
import { quirkFull } from './quirk-full.mjs';

const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const { writeDefaultTabStop, writeHyphenationSettings, writeEvenAndOddHeaders, writeDisplayBackgroundShape } = await ts('settings-xml.js');
const { ensureNoteParts, patchSettingsNotes } = await ts('notes.js');
const { hfPartXml, copyHfPartXml, dropUnusedHfParts, pageNumberGroupIds } = await ts('hf-ops.js');

const sha = (u8) => createHash('sha256').update(u8).digest('hex').slice(0, 32);
const enc = (s) => new TextEncoder().encode(s);
const dec = (u8) => new TextDecoder().decode(u8);
const SETTINGS = 'word/settings.xml';
const RELS = 'word/_rels/document.xml.rels';
const CTYPES = '[Content_Types].xml';
const W_NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"';

// the parts the functions look at
const keep = (n) => n === CTYPES || n === RELS || n === SETTINGS || /^word\/(?:footnotes|endnotes)\.xml$/.test(n)
  || /^word\/(?:header|footer)[^/]*\.xml$/.test(n) || /^word\/_rels\/(?:header|footer)[^/]*\.rels$/.test(n);

const edit = (parts, name, f) => parts.map(([n, d]) => (n === name ? [n, enc(f(dec(d)))] : [n, d]));
const drop = (parts, name) => parts.filter(([n]) => n !== name);
const has = (parts, name) => parts.some(([n]) => n === name);
const ELEMENTS = ['w:defaultTabStop', 'w:autoHyphenation', 'w:consecutiveHyphenLimit', 'w:hyphenationZone', 'w:doNotHyphenateCaps',
  'w:evenAndOddHeaders', 'w:displayBackgroundShape', 'w:footnotePr', 'w:endnotePr'];
const stripElements = (t) => ELEMENTS.reduce((x, n) => x.replace(new RegExp(`<${n}\\b[^>]*?(?:/>|>[\\s\\S]*?</${n}>)`, 'g'), ''), t);
const VARIANTS = {
  asis: (p) => p,
  nosettings: (p) => drop(p, SETTINGS),
  norel: (p) => edit(p, RELS, (t) => t.replace(/<Relationship\b[^>]*relationships\/settings"[^>]*\/>/g, '')),
  nosettings_norel: (p) => edit(drop(p, SETTINGS), RELS, (t) => t.replace(/<Relationship\b[^>]*relationships\/settings"[^>]*\/>/g, '')),
  selfclose: (p) => edit(p, SETTINGS, () => `<w:settings ${W_NS}/>`),
  bare: (p) => edit(p, SETTINGS, () => `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings ${W_NS}></w:settings>`),
  space: (p) => edit(p, SETTINGS, () => `<w:settings ${W_NS}> </w:settings>`),
  strip: (p) => edit(p, SETTINGS, stripElements),
  off: (p) => edit(p, SETTINGS, (t) => stripElements(t).replace('</w:settings>', '<w:evenAndOddHeaders w:val="false"/><w:displayBackgroundShape w:val="off"/></w:settings>')),
  zero: (p) => edit(p, SETTINGS, (t) => stripElements(t).replace('</w:settings>', '<w:evenAndOddHeaders w:val="0"></w:evenAndOddHeaders><w:displayBackgroundShape/><w:defaultTabStop w:val="1e+21"/><w:autoHyphenation w:val="0"/></w:settings>')),
  prefixed: (p) => edit(p, SETTINGS, (t) => t.replace(/xmlns:w=/, 'xmlns:ns0=').replace(/<(\/?)w:/g, '<$1ns0:').replace(/ w:/g, ' ns0:')),
  bom: (p) => p.map(([n, d]) => (n === SETTINGS ? [n, new Uint8Array([0xef, 0xbb, 0xbf, ...d])] : [n, d])),
  noct: (p) => drop(p, CTYPES),
  emptyct: (p) => p.map(([n, d]) => (n === CTYPES ? [n, new Uint8Array()] : [n, d])),
  emptyrels: (p) => p.map(([n, d]) => (n === RELS ? [n, new Uint8Array()] : [n, d])),
  badxml: (p) => edit(p, SETTINGS, (t) => t.replace('</w:settings>', '')),
  oneline: (p) => edit(p, SETTINGS, (t) => t.replace(/>\s*</g, '><')),
};
const sameParts = (a, b) => a.length === b.length && a.every(([n, d], i) => n === b[i][0] && d.length === b[i][1].length && d.every((x, k) => x === b[i][1][k]));

const fromParts = (parts) => {
  const pkg = DocxPackage.empty();
  for (const n of pkg.partNames()) pkg.removePart(n);
  for (const [n, d] of parts) pkg.setPart(n, d);
  return pkg;
};
const snapshot = (pkg) => pkg.partNames().map((n) => [n, pkg.getPart(n)]);
const summary = (before, after) => {
  const b = new Map(before);
  const lines = [`order ${after.map(([n]) => n).join('|')}`];
  for (const [n, d] of after) {
    const o = b.get(n);
    if (!o || o.length !== d.length || !o.every((x, i) => x === d[i])) lines.push(`${n}=${sha(d)}`);
  }
  return lines.join('\n');
};

const HYPH = [
  { auto: true, caps: true, limit: 0 },
  { auto: false, caps: false, zone: 360, limit: 3 },
  { auto: true, caps: false, zone: 0, limit: 2.5 },
  { auto: false, caps: true, limit: 0 },
  { auto: true, caps: true, zone: -1.5, limit: 1e21 },
  { auto: false, caps: true, limit: NaN, zone: NaN },
];
const TABS = [720, 567.4, 0, -0.4, 1e21, NaN];
const NOTE_PROPS = [
  ['keep', {}],
  ['set', { footnote: { pos: 'pageBottom', numFmt: 'decimal', numStart: 1, numRestart: 'continuous' }, endnote: { pos: 'docEnd' } }],
  ['remove', { footnote: null, endnote: null }],
  ['mixed', { footnote: { numStart: 0 }, endnote: null }],
];

const out = new CaptureWriter([
  'D1b W4/W4b: settings-xml.ts, notes.ts (ensureNoteParts, patchSettingsNotes), hf-ops.ts over the corpus mini packages x variants x arguments',
  'facets: out (short sha256 of the summary, or err), args, ids:<story>',
]);
let n = 0;
const explain = process.env.UO_EXPLAIN;
const textSummaries = new Map();
function pkgOp(key, parts, f) {
  const pkg = fromParts(parts);
  const before = snapshot(pkg);
  let text;
  try {
    f(pkg);
    text = summary(before, snapshot(pkg));
  } catch {
    text = 'err';
  }
  put(key, text);
}
function pureOp(key, f) {
  let text;
  try { text = f(); } catch { text = 'err'; }
  put(key, text);
}
function put(key, text) {
  if (explain === key) console.log(text);
  out.set(key, 'out', text === 'err' ? 'err' : sha(enc(text)));
  n++;
}

// ── the distinct mini packages of the corpus ──
const seen = new Map();
const inputs = ['# settings-xml inputs (js/capture-settings-xml.mjs): `@input <docx>` then `<part>\\t<base64>` per part'];
const docs = [];
for (const { rel, file } of corpus()) {
  let pkg;
  try { pkg = DocxPackage.load(readBytes(file)); } catch { continue; }
  const parts = pkg.partNames().filter(keep).map((name) => [name, pkg.getPart(name)]);
  const h = createHash('sha256');
  for (const [name, d] of parts) { h.update(`${name}\0`); h.update(d); }
  const k = h.digest('hex');
  if (seen.has(k)) continue;
  seen.set(k, rel);
  docs.push({ rel, parts });
  inputs.push(`@input ${rel}`);
  for (const [name, d] of parts) inputs.push(`${name}\t${Buffer.from(d).toString('base64')}`);
}

docs.forEach(({ rel, parts }, di) => {
  // every package as it is; the variants on every 4th (the rest of the matrix is the same code paths)
  for (const [vname, vf] of Object.entries(VARIANTS)) {
    if (vname !== 'asis' && di % 4 !== 0) continue;
    const vparts = vf(parts);
    if (vname !== 'asis' && sameParts(vparts, parts)) continue;
    const base = `${rel}#${vname}`;
    for (const tw of TABS) pkgOp(`${base}#tab:${String(tw)}`, vparts, (p) => writeDefaultTabStop(p, tw));
    HYPH.forEach((s, i) => pkgOp(`${base}#hyph:${i}`, vparts, (p) => writeHyphenationSettings(p, s)));
    for (const on of [true, false]) {
      pkgOp(`${base}#even:${on}`, vparts, (p) => writeEvenAndOddHeaders(p, on));
      pkgOp(`${base}#bg:${on}`, vparts, (p) => writeDisplayBackgroundShape(p, on));
    }
    pkgOp(`${base}#ensure`, vparts, (p) => { out.set(`${base}#ensure`, 'ret', String(ensureNoteParts(p))); });
    const settings = vparts.find(([nm]) => nm === SETTINGS);
    if (settings) for (const [pn, props] of NOTE_PROPS) pureOp(`${base}#patch:${pn}`, () => patchSettingsNotes(dec(settings[1]), props));
    const rels = vparts.find(([nm]) => nm === RELS);
    const ids = [];
    if (rels) {
      for (const m of dec(rels[1]).matchAll(/<Relationship\b[^>]*?\/>/g)) {
        if (!/relationships\/(?:header|footer)"/.test(m[0])) continue;
        const id = /\bId="([^"]+)"/.exec(m[0])?.[1];
        if (id) ids.push(id);
      }
    }
    const sets = [['all', ids], ['first', ids.slice(0, 1)], ['last', ids.slice(-1)], ['none', ['rIdNope']]];
    for (const [sn, set] of sets) {
      out.set(`${base}#drop:${sn}`, 'args', set.join('|'));
      pkgOp(`${base}#drop:${sn}`, vparts, (p) => dropUnusedHfParts(p, new Set(set)));
    }
    for (const [name, d] of vparts) {
      if (!/^word\/(?:header|footer)[^/]*\.xml$/.test(name)) continue;
      pureOp(`${base}#copy:${name}`, () => { let c = 100; return copyHfPartXml(dec(d), () => ++c); });
    }
  }
});
for (const kind of ['header', 'footer']) {
  ['', '<w:p/>', '<w:p><w:r><w:t>é😀&amp;</w:t></w:r></w:p>'].forEach((inner, i) => pureOp(`hfpart#${kind}#${i}`, () => hfPartXml(kind, inner)));
}

// ── pageNumberGroupIds: the story sources of the synthetic packages and the corpus documents with galleries ──
const fromPartsFull = (parts) => {
  const pkg = DocxPackage.empty();
  if (!parts.some(([path]) => path === 'word/document.xml')) pkg.removePart('word/document.xml');
  for (const [path, data] of parts) pkg.setPartText(path, data);
  return pkg;
};
function pageNumbers(key, pkg) {
  let full;
  try { full = readFullDocument(pkg, { sources: true }); } catch { return; }
  const stories = full.sources?.stories;
  if (!stories) return;
  for (const [path, story] of stories) {
    if (!/^word\/(?:header|footer)/.test(path)) continue;
    const flat = [];
    const walk = (groups) => { for (const g of groups) { for (const id of g.blocks) flat.push(id); if (g.inner) walk(g.inner); } };
    walk(story.groups);
    const ids = [...pageNumberGroupIds(story)];
    out.set(`pn#${key}`, `ids:${path}`, JSON.stringify(ids.map((id) => flat.indexOf(id))));
    n++;
  }
}
for (const { key, parts } of [...quirkSources(QUIRK_SEED, QUIRK_COUNT), ...quirkFull()]) {
  if (!parts.some(([path, data]) => /^word\/(?:header|footer)/.test(path) && String(data).includes('docPartGallery'))) continue;
  pageNumbers(key, fromPartsFull(parts));
}
for (const { rel, file } of corpus()) {
  let pkg;
  try { pkg = DocxPackage.load(readBytes(file)); } catch { continue; }
  if (!pkg.partNames().some((name) => /^word\/(?:header|footer)[^/]*\.xml$/.test(name) && dec(pkg.getPart(name)).includes('docPartGallery'))) continue;
  pageNumbers(rel, pkg);
}

writeFileSync(join(crate, 'tests', 'parity', 'settings-xml.inputs'), inputs.join('\n') + '\n');
const path = join(crate, 'tests', 'parity', 'settings-xml.capture');
out.write(path);
console.log(`settings-xml: ${docs.length} distinct packages, ${n} records -> ${path}`);
