// Capture of frontier R4b (inline objects, facet `drawing-runs`) from the frozen TS engine:
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-drawing-runs.mjs
//     [--only <substring>] [--trace <key>]
//
// Inputs: every git-tracked .docx/.docm/.dotx of the checkout (the 2,742 oracle documents of
// docx-layout and the other fixtures) and the seeded generated corpus of `js/quirk-objects.mjs`
// (tests/parity/drawing-runs.quirks.txt, replayed by Rust as it is). Per XML part (`word/*.xml`),
// every `w:drawing`, `w:pict`, `w:object` and `mc:AlternateContent` in document order is one
// record `<docx path>#<part>#<ordinal>` (quirk cases: `quirk/obj-<n>#fragment#<ordinal>`), with
// the facets (each also as `<facet>.o`, the key-order facet of the canonical digest):
//
//   runs     the object runs `readParagraph` makes of `<w:p><w:r>{element}</w:r></w:p>`, as
//            `[{ text, image?, textbox?, shape?, group? }]` (the props are R4a's/R4c's)
//   fmt      w:drawing -> readPictureFormat(raw)
//   fmtany   w:drawing -> readPictureFormat(raw, { anyShape: true })
//   spec     w:drawing -> for every element below it (document order): dmlColorSpec(readDmlColor(el)) or U
//   sync     w:drawing -> for each of 9 edits of the raw text (`variants`), the objects of the
//            runs after syncImageFromXml / syncShapeFromXml with that raw text
//
// The context is the one `buildReadContext` makes for the part (real documents) or the quirk
// rules (a relationship id resolves to an image when its length is even, to the part carrying
// that id otherwise), with a stub `readBlocks`: the blocks of a text box content are one
// paragraph `{ runs: [{ text: <its text>, props: {} }], props: {} }` per `w:p` child (R5 owns
// the real block reader). Two key positions the model fixes by design are moved on the TS side
// (record R4b item 4): `props` of a group item's run goes after `text` and before the object key,
// and `paras`/`blocks` of a text box go after the object's own keys.
// A facet is the SHA-256 (first 16 bytes) of the canonical walk (js/canon.mjs); a function that
// throws is the string '!throw'. Output: tests/parity/drawing-runs.capture.
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Stream, importFrom, tsRoot } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';
import { readCorpus } from './quirk-objects.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = tsRoot();
const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : null;
const traceKey = argv.includes('--trace') ? argv[argv.indexOf('--trace') + 1] : null;

const core = 'write/packages/docx-core/dist/';
const { DocxPackage } = await importFrom(root, `${core}package.js`);
const { parseXml, childElements, textContent } = await importFrom(root, `${core}xml.js`);
const Doc = await importFrom(root, `${core}document.js`);
const { readPictureFormat, readDmlColor, dmlColorSpec } = await importFrom(root, `${core}picture-xml.js`);
const tsCommit = execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();

const TARGETS = new Set(['w:drawing', 'w:pict', 'w:object', 'mc:AlternateContent']);
const KEYS = ['image', 'textbox', 'shape', 'group'];
const enc = new TextEncoder();

/** The nine edits of a raw `w:drawing` the sync facet applies (the Rust test has the same list). */
export function variants(raw) {
  return [
    raw,
    raw.replace('<wp:inline', '<wp:anchor').replace('</wp:inline>', '</wp:anchor>'),
    raw.replace('<wp:anchor', '<wp:inline').replace('</wp:anchor>', '</wp:inline>'),
    raw.replace('<a:xfrm>', '<a:xfrm rot="2700000" flipH="1" flipV="1">'),
    raw.replace('<wp:docPr ', '<wp:docPr hidden="1" '),
    raw.replace('<a:prstGeom prst="rect"', '<a:prstGeom prst="ellipse"'),
    raw.replace('<a:ext cx="', '<a:ext cx="9'),
    raw.replace('<wp:extent cx="', '<wp:extent cx="3'),
    raw.replace('behindDoc="0"', 'behindDoc="1"'),
  ];
}

/** The stub block reader: one paragraph per `w:p` child. */
function readBlocks(container) {
  return childElements(container, 'w:p').map((p) => ({ type: 'paragraph', paragraph: { runs: [{ text: textContent(p), props: {} }], props: {} } }));
}

/** The model keeps the content of a text box (`paras`, `blocks`) after the object's own keys. */
function normTextbox(tb) {
  const { paras, blocks, ...rest } = tb;
  return blocks ? { ...rest, paras, blocks } : { ...rest, paras };
}

/** `{ ...run, props }` of a group item as the model orders it: text, props, then the rest. */
function reorderItemRun(r) {
  const { text, props, ...rest } = r;
  const out = { text, props, ...rest };
  if (out.group) out.group = normGroup(out.group);
  if (out.textbox) out.textbox = normTextbox(out.textbox);
  return out;
}
function normGroup(g) {
  return { ...g, items: g.items.map((it) => ({ ...it, run: reorderItemRun(it.run) })) };
}

/** The object part of runs, with the group items' runs reordered. */
function objsOf(runs) {
  return runs.map((r) => {
    const o = { text: r.text };
    for (const k of KEYS) if (r[k] !== undefined) o[k] = k === 'group' ? normGroup(r[k]) : k === 'textbox' ? normTextbox(r[k]) : r[k];
    return o;
  });
}

const digest = (fn, trace) => {
  const s = new Stream({ trace: !!trace });
  const o = new Stream({ trace: !!trace });
  try { fn(new Canon(s, { order: o })); } catch {
    const t = new Stream();
    const to = new Stream();
    new Canon(t, { order: to }).str('!throw');
    return { m: t.digest().short, o: to.digest().short };
  }
  return { m: s.digest().short, o: o.digest().short, tm: s.trace(), to: o.trace() };
};

function runsOf(el, ctx) {
  const p = { name: 'w:p', attrs: {}, children: [{ name: 'w:r', attrs: {}, children: [el] }] };
  return Doc.readParagraph(p, { ...ctx, activeComments: new Set(), fieldStack: [] }).runs;
}

function facetsOf(el, ctx) {
  const f = [];
  f.push(['runs', (c) => c.value(objsOf(runsOf(el, ctx)), 'x')]);
  if (el.name === 'w:drawing') {
    const raw = el.raw;
    f.push(['fmt', (c) => c.value(readPictureFormat(raw), 'x')]);
    f.push(['fmtany', (c) => c.value(readPictureFormat(raw, { anyShape: true }), 'x')]);
    f.push(['spec', (c) => {
      const st = [el];
      const all = [];
      while (st.length) { const e = st.pop(); all.push(e); const k = childElements(e); for (let i = k.length - 1; i >= 0; i--) st.push(k[i]); }
      for (const e of all) { const col = readDmlColor(e); if (col) c.value(dmlColorSpec(col), 'x'); else c.s.tag('U'); }
    }]);
    f.push(['sync', (c) => {
      variants(raw ?? '').forEach((v, i) => {
        c.s.tag('V').u32(i);
        const runs = runsOf(el, ctx);
        for (const run of runs) {
          if (run.image?.rawDrawingXml) { run.image.rawDrawingXml = v; Doc.syncImageFromXml(run.image); }
          else if (run.textbox?.rawDrawingXml) { run.textbox.rawDrawingXml = v; Doc.syncShapeFromXml(run); }
          else if (run.shape?.rawDrawingXml) { run.shape.rawDrawingXml = v; Doc.syncShapeFromXml(run); }
          else if (run.group?.rawDrawingXml) { run.group.rawDrawingXml = v; Doc.syncShapeFromXml(run); }
        }
        c.value(objsOf(runs), 'x');
      });
    }]);
  }
  return f;
}

function targetsOf(rootEl) {
  const out = [];
  const stack = [rootEl];
  while (stack.length) {
    const el = stack.pop();
    if (TARGETS.has(el.name)) out.push(el);
    const kids = childElements(el);
    for (let i = kids.length - 1; i >= 0; i--) stack.push(kids[i]);
  }
  return out;
}

const cap = new CaptureWriter(['uo-docx R4b drawing-runs facet: the object runs of w:drawing/w:pict/w:object/mc:AlternateContent, readPictureFormat, syncImageFromXml/syncShapeFromXml']);
cap.meta('ts-commit', tsCommit);
let records = 0;
let docs = 0;

function record(keyBase, parts) {
  // parts: [partName, text, ctx]
  let ordinal = 0;
  for (const [name, text, ctx] of parts) {
    let rootEl;
    try { rootEl = parseXml(text); } catch { continue; }
    for (const el of targetsOf(rootEl)) {
      const key = `${keyBase}#${name}#${ordinal++}`;
      if (only && !key.includes(only)) continue;
      for (const [facet, fn] of facetsOf(el, ctx)) {
        const d = digest(fn, traceKey === key);
        cap.set(key, facet, d.m);
        cap.set(key, `${facet}.o`, d.o);
        if (traceKey === key) console.log(`--- ${key} ${facet}\n${d.tm}\n--- order\n${d.to}`);
      }
      records++;
    }
  }
  cap.set(keyBase, 'targets', String(ordinal));
}

// ── the real documents ──
const files = execFileSync('git', ['-C', root, 'ls-files', '*.docx', '*.docm', '*.dotx'], { encoding: 'utf8', maxBuffer: 1 << 26 }).split('\n').filter(Boolean).sort();
for (const rel of files) {
  let pkg;
  try { pkg = DocxPackage.load(new Uint8Array(readFileSync(join(root, rel)))); } catch { continue; }
  const parts = [];
  for (const name of pkg.partNames()) {
    if (!/^word\/[^/]*\.xml$/.test(name)) continue;
    const text = pkg.getPartText(name);
    if (text === undefined) continue;
    const relsPath = `word/_rels/${name.slice(5)}.rels`;
    let ctx;
    try { ctx = { ...Doc.buildReadContext(pkg, relsPath), readBlocks }; } catch { continue; }
    parts.push([name, text, ctx]);
  }
  docs++;
  record(rel, parts);
}

// ── the generated attribute-quirk corpus ──
const quirks = readCorpus();
for (const [key, frag, partList] of quirks) {
  const partMap = new Map(partList);
  const ctx = {
    resolveImage: (rid) => (rid.length % 2 === 0 ? { data: enc.encode(`img:${rid}`), ext: 'png' } : undefined),
    resolveLink: () => undefined,
    resolvePart: (rid) => { const t = partMap.get(rid); return t === undefined ? undefined : parseXml(t); },
    readBlocks,
  };
  record(key, [['fragment', frag, ctx]]);
}

const outDir = resolve(here, '../tests/parity');
cap.meta('corpus', `${docs} docx, ${quirks.length} quirk cases, ${records} records`);
if (!only && !traceKey) cap.write(join(outDir, 'drawing-runs.capture'));
console.log(`${docs} docx, ${quirks.length} quirk cases, ${records} records`);
