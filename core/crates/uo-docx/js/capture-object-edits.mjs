// Parity capture of the drawing-object emitters of D1b (frontier W2: `objectXml`'s edited branches,
// `editedShapeXml`, `editedGroupXml`, `pictureXml`, `drawingXml`, `oleObjectXml`, `onceShape`,
// `beginShapeContext`, `patchDrawingGeometry` on the document path): docx-core's lossless harness (the
// `open` + `save` of capture-write-roundtrip.mjs) with one named edit applied to the top-level
// paragraphs of the body between the install (the baseline) and the save — what the editor's operations
// will do (set `edited`, resize, hide a group, swap an image) — over the corpus documents that have
// objects, in two variants: `all` (read with sources, every header/footer part dirty) and `gen` (read
// without sources: the generating path).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-object-edits.mjs
//
// Writes tests/parity/object-edits.capture, one record per `<docx>#<edit>#<variant>` with the facets of
// capture-write-roundtrip.mjs (`error`, `order`, `changed`, `p:<part>`); the edits are implemented again
// by tests/parity_object_edits.rs (same names, same effect).
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { corpus, crate, readBytes, ts } from './capture-r2-common.mjs';

const { DocxPackage } = await ts('package.js');
const { readFullDocument, markSectionBreaks } = await ts('fulldoc.js');
const { writeDocument } = await ts('serialize.js');

const sha = (u8) => createHash('sha256').update(u8).digest('hex').slice(0, 32);

// ── lossless.test.ts:15–49 (verbatim but for the imports) ──
function open(pkg, sources = true) {
  const original = undefined;
  const full = readFullDocument(pkg, { sources });
  markSectionBreaks(full);
  full.sources?.baseline();
  return { pkg, full, original };
}

function save(pkg, full, dirtyHf = []) {
  const last = full.sections[full.sections.length - 1];
  const hfParts = [];
  const seen = new Set();
  for (const s of full.sections) {
    for (const [kind, refs, set, blocks] of [
      ['header', s.headerRefs, s.headers, s.headerBlocks],
      ['footer', s.footerRefs, s.footers, s.footerBlocks],
    ]) {
      for (const r of refs) {
        if (seen.has(r.path) || !set[r.type]) continue;
        seen.add(r.path);
        hfParts.push({ kind, type: r.type, rid: r.rid, path: r.path, paragraphs: set[r.type], blocks: blocks?.[r.type], dirty: dirtyHf.includes(r.path) });
      }
    }
  }
  const lastRefs = [...last.headerRefs, ...last.footerRefs].map((r) => r.path);
  writeDocument(pkg, { paragraphs: paragraphsOf(full.blocks), sectPr: last.sectPr }, hfParts, {
    comments: full.comments.length ? full.comments : undefined,
    bodySectRefs: hfParts.filter((h) => lastRefs.includes(h.path)).map((h) => ({ kind: h.kind, type: h.type, rid: h.rid })),
    blocks: full.blocks,
    sources: full.sources,
  });
  return DocxPackage.load(pkg.save());
}

const paragraphsOf = (blocks) => blocks.filter((b) => b.type === 'paragraph').map((b) => b.paragraph);

// ── the named edits: each returns whether it touched the run ──
const grow = (o) => {
  o.widthTwips += 100;
  o.heightTwips += 50;
  if (o.offsetXTwips !== undefined) o.offsetXTwips += 10;
  if (o.offsetYTwips !== undefined) o.offsetYTwips -= 20;
};
export const EDITS = [
  ['shapes', (r) => { let t = false; for (const o of [r.textbox, r.shape]) if (o && o.rawDrawingXml) { o.edited = true; t = true; } return t; }],
  ['shapes-size', (r) => { let t = false; for (const o of [r.textbox, r.shape]) if (o && o.rawDrawingXml) { o.edited = true; grow(o); t = true; } return t; }],
  ['vml-boxes', (r) => { if (r.textbox) { r.textbox.vml = true; return true; } return false; }],
  ['groups', (r) => { if (r.group?.rawDrawingXml) { r.group.edited = true; return true; } return false; }],
  ['groups-hide', (r) => {
    const g = r.group;
    if (!g?.rawDrawingXml) return false;
    g.edited = true;
    g.rawDrawingXml = g.rawDrawingXml.replace('<wp:docPr ', '<wp:docPr hidden="1" ').replace('behindDoc="0"', 'behindDoc="1"');
    return true;
  }],
  ['images', (r) => { if (r.image?.rawDrawingXml) { r.image.edited = true; return true; } return false; }],
  ['images-size', (r) => { if (r.image?.rawDrawingXml) { r.image.edited = true; grow(r.image); return true; } return false; }],
  ['images-swap', (r) => {
    const i = r.image;
    if (!i?.rawDrawingXml) return false;
    i.edited = true;
    i.rawDrawingXml = i.rawDrawingXml.replace(/r:embed="[^"]*"/, 'r:embed="uw:media"');
    return true;
  }],
  ['noraw', (r) => { if (r.image) { delete r.image.rawDrawingXml; return true; } return false; }],
  ['noraw-ole', (r) => {
    if (!r.image) return false;
    delete r.image.rawDrawingXml;
    r.image.ole = { progId: 'Excel.Sheet.12', data: r.image.data, ext: 'xlsx' };
    return true;
  }],
];

const out = new CaptureWriter([
  'D1b object emitters: docx-core lossless harness (open + named edit of the body paragraphs + save), parts compared decompressed',
  'facets: error | order, changed, p:<part> (short sha256 of each changed part)',
]);
let n = 0;
let touched = 0;
for (const { rel, file } of corpus()) {
  const bytes = readBytes(file);
  for (const [name, edit] of EDITS) {
    for (const variant of ['all', 'gen']) {
      let input;
      let pkg;
      let full;
      try {
        input = DocxPackage.load(bytes);
        ({ pkg, full } = open(DocxPackage.load(bytes), variant === 'all'));
      } catch { continue; }
      let hit = 0;
      for (const b of full.blocks) if (b.type === 'paragraph') for (const r of b.paragraph.runs) if (edit(r)) hit++;
      if (!hit) continue;
      touched += hit;
      const key = `${rel}#${name}#${variant}`;
      try {
        const dirty = full.sections.flatMap((s) => [...s.headerRefs, ...s.footerRefs].map((r) => r.path));
        const after = save(pkg, full, dirty);
        const names = after.partNames();
        out.set(key, 'order', sha(new TextEncoder().encode(names.join(String.fromCharCode(10)))));
        const changed = [];
        for (const nm of names) {
          const a = after.getPart(nm);
          const b = input.getPart(nm);
          if (!b || a.length !== b.length || !a.every((x, i) => x === b[i])) changed.push(nm);
        }
        if (changed.some((c) => /[\t\n\r|=]/.test(c))) throw new Error('part name not representable in the capture');
        out.set(key, 'changed', changed.join('|'));
        for (const c of changed) out.set(key, `p:${c}`, sha(after.getPart(c)));
      } catch {
        out.set(key, 'error', 'err');
      }
      n++;
    }
  }
}
const path = join(crate, 'tests', 'parity', 'object-edits.capture');
out.write(path);
console.log(`object-edits: ${n} records (${touched} runs edited) -> ${path}`);
