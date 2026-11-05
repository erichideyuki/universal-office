// Parity capture of D2b-F2's pure model files (record uo-docx-d2b/F2.md, "Parity"): the frozen TS
// transformations of the style sheet text (editor/src/styles.ts over docx-core style-sheet.ts and
// word-styles.ts: ensureDefined, createStyleXml, modifyStyleXml, renameStyleXml, deleteStyleXml,
// setQuickXml, resolveStyleId) and of the numbering part (editor/src/numbering-store.ts
// `DocNumbering`: addList, addNum, copyAbstract, setLevel, setLevels, withTentative, with the
// library of list-library.ts), over the `word/styles.xml` / `word/numbering.xml` of corpus
// documents. Node 24.15 x64 (`Math.random` = the testkit xorshift32, seed 1 per document).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f2-ops.mjs [--docs N] [--ops N]
//
// Writes tests/parity/d2b-f2-ops.capture: per `<doc>#s<j>` (style op j) and `<doc>#n<j>` (numbering
// op j) the facets `op` (encoded; the Rust replay runs it), `ret` (what it returned, or `err`) and
// `xml` (the short SHA-256 of the text after it; `out` for withTentative's result).
import { createHash } from 'node:crypto';
import { CaptureWriter, Rng, importFrom } from '../../uo-testkit/js/testkit.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';
import { join } from 'node:path';

const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 200);
const OPS = opt('--ops', 40);
const sha = (s) => createHash('sha256').update(Buffer.from(s, 'utf8')).digest('hex').slice(0, 32);

const { DocxPackage } = await ts('package.js');
const ss = await ts('style-sheet.js');
const st = await importFrom(root, 'write/packages/editor/src/styles.ts');
const nb = await importFrom(root, 'write/packages/editor/src/numbering-store.ts');
const lib = await importFrom(root, 'write/packages/editor/src/list-library.ts');

let rs = 1;
Math.random = () => { rs ^= rs << 13; rs >>>= 0; rs ^= rs >>> 17; rs ^= rs << 5; rs >>>= 0; return rs / 4294967296; };

const LOC = { lcid: '0416', heading: (n) => `Ttulo${n}`, article: 'Artigo', section: 'Seção', chapter: 'Capítulo' };
const LIBS = [lib.BULLET_LIBRARY, lib.NUMBER_LIBRARY, lib.MULTILEVEL_LIBRARY];
const NAMES = ['Meu Estilo', 'Destaque X', 'Citação', 'Nota 1', 'Ttulo 1', 'heading 1'];
const FORMATS = [
  {}, { bold: true, szHalfPt: 28 }, { alignment: 'center', spaceBeforeTwips: 120, lineTwentieths: 360 },
  { fontAscii: 'Arial', color: 'FF0000', italic: true, underline: true }, { indentLeftTwips: 720, indentFirstLineTwips: -360, charSpacingTwips: 20 },
  { bold: null, fontAscii: null, alignment: null, indentFirstLineTwips: null },
];

const out = new CaptureWriter([
  'D2b-F2 pure model files: styles.ts / style-sheet.ts transformations and numbering-store.ts DocNumbering over corpus parts',
  'facets: op, ret, xml',
]);
out.meta('ops', OPS);

const files = corpus().filter((_, i) => i % 7 === 3).slice(0, DOCS);
let n = 0;
for (const { rel, file } of files) {
  let pkg;
  try { pkg = DocxPackage.load(readBytes(file)); } catch { continue; }
  let seed = 0xf20;
  for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
  const rng = new Rng(seed);
  const pick = (a) => a[rng.below(a.length)];
  rs = 1;
  // ── the style sheet ──
  let xml = pkg.getPartText('word/styles.xml');
  if (xml) {
    const builtinIds = ss.builtinStyles().map((b) => /w:styleId="([^"]*)"/.exec(b.xml)[1]);
    for (let j = 0; j < OPS; j++) {
      const idx = ss.readStyleIndex(xml);
      const ids = [...idx.styles.map((s) => s.id), ...builtinIds];
      const customs = idx.styles.filter((s) => s.custom).map((s) => s.id);
      const k = rng.below(100);
      let code;
      if (k < 25) code = `ens:${pick(ids)}:${rng.below(2)}`;
      else if (k < 45) code = `cre:${rng.below(NAMES.length)}:${pick(['linked', 'paragraph', 'character'])}:${rng.below(FORMATS.length)}:${rng.below(3) === 0 ? pick(ids) : '-'}`;
      else if (k < 62) code = `mod:${pick(ids)}:${rng.below(FORMATS.length)}:${rng.below(4)}`;
      else if (k < 72) code = `ren:${pick(customs.length && rng.below(3) ? customs : ids)}:${rng.below(NAMES.length)}`;
      else if (k < 82) code = `del:${pick(customs.length && rng.below(3) ? customs : ids)}`;
      else if (k < 94) code = `qk:${pick(ids)}:${rng.below(2)}`;
      else code = `rsv:${pick(ids)}`;
      const f = code.split(':');
      const ds = new st.DocStyles(xml);
      let ret = '-';
      try {
        switch (f[0]) {
          case 'ens': xml = st.ensureDefined(ds, xml, f[1], f[2] === '1'); break;
          case 'cre': { const r = st.createStyleXml(ds, xml, { name: NAMES[+f[1]], type: f[2], format: FORMATS[+f[3]], ...(f[4] !== '-' ? { basedOn: f[4] } : {}) }); xml = r.xml; ret = r.id; break; }
          case 'mod': {
            const o = [{}, { quick: false, autoUpdate: true }, { next: null }, { name: NAMES[+f[3] + 1], quick: true }][+f[3]];
            const r = st.modifyStyleXml(ds, xml, f[1], { ...o, format: FORMATS[+f[2]] }); xml = r.xml; ret = JSON.stringify(r.renamed); break;
          }
          case 'ren': { const r = st.renameStyleXml(ds, xml, f[1], NAMES[+f[2]]); xml = r.xml; ret = JSON.stringify(r.renamed); break; }
          case 'del': { const r = st.deleteStyleXml(xml, f[1]); if (r) { xml = r.xml; ret = JSON.stringify(r.removed); } else ret = 'null'; break; }
          case 'qk': xml = st.setQuickXml(ds, xml, f[1], f[2] === '1'); break;
          case 'rsv': ret = st.resolveStyleId(xml, f[1]); break;
        }
      } catch { ret = 'err'; }
      const key = `${rel}#s${j}`;
      out.set(key, 'op', code);
      out.set(key, 'ret', ret);
      out.set(key, 'xml', sha(xml));
    }
  }
  // ── the numbering part ──
  rs = 1;
  const num = new nb.DocNumbering(pkg.getPartText('word/numbering.xml') ?? null);
  const touched = new Set();
  for (let j = 0; j < OPS; j++) {
    const abstracts = [...(num.xml ?? '').matchAll(/<w:abstractNum\b[^>]*\bw:abstractNumId="(\d+)"/g)].map((m) => +m[1]);
    const absId = () => (abstracts.length ? pick(abstracts) : 0);
    const k = rng.below(100);
    let code;
    if (k < 25) code = `al:${rng.below(3)}:${rng.below(6)}`;
    else if (k < 38) code = `an:${absId()}:${rng.below(2)}`;
    else if (k < 52) code = `ca:${absId()}`;
    else if (k < 68) code = `sl:${absId()}:${rng.below(9)}:${rng.below(3)}:${rng.below(6)}`;
    else if (k < 82) code = `sm:${absId()}:${rng.below(3)}:${rng.below(6)}`;
    else code = `wt:${absId()}:${rng.below(4)}`;
    const f = code.split(':');
    let ret = '-';
    const def = (l, i) => LIBS[l][i % LIBS[l].length];
    try {
      switch (f[0]) {
        case 'al': { const d = def(+f[1], +f[2]); ret = String(num.addList(nb.libraryLevels(d, LOC), { multiLevelType: d.multiLevelType, ...(d.tmpl ? { tmpl: d.tmpl.replace('{LCID}', LOC.lcid) } : {}) })); touched.add(num.abstractOf(+ret)); break; }
        case 'an': ret = String(num.addNum(+f[1], f[2] === '1' ? '<w:lvlOverride w:ilvl="0"><w:startOverride w:val="3"/></w:lvlOverride>' : '')); break;
        case 'ca': { const r = num.copyAbstract(+f[1]); ret = String(r); if (r !== undefined) touched.add(r); break; }
        case 'sl': { const lv = nb.libraryLevels(def(+f[3], +f[4]), LOC)[+f[2]] ?? ''; num.setLevel(+f[1], +f[2], lv.replace(/w:ilvl="\d+"/, `w:ilvl="${f[2]}"`)); touched.add(+f[1]); break; }
        case 'sm': { const d = def(+f[2], +f[3]); num.setLevels(+f[1], nb.libraryLevels(d, LOC), { multiLevelType: 'multilevel', tmpl: '01234567' }); touched.add(+f[1]); break; }
        case 'wt': {
          const used = new Map(abstracts.map((a) => [a, new Set(f[2] === '0' ? [] : f[2] === '1' ? [0] : f[2] === '2' ? [0, 1, 2] : [8])]));
          ret = sha(num.withTentative(touched, used) ?? '');
          break;
        }
      }
    } catch { ret = 'err'; }
    const key = `${rel}#n${j}`;
    out.set(key, 'op', code);
    out.set(key, 'ret', ret);
    out.set(key, 'xml', num.xml === null ? '-' : sha(num.xml));
  }
  n++;
}
out.write(join(crate, 'tests', 'parity', 'd2b-f2-ops.capture'));
console.log(`d2b-f2-ops: ${n} documents x ${OPS} ops (styles and numbering)`);
