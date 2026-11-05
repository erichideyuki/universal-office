// Parity capture of the ODF port (D5b, `odf/**`): the frozen TS `writeOdt` and `readOdt` of docx-core
// (write/packages/docx-core/src/odf/), over
//   - the 2,753 docx of the oracle corpus and the docx-core fixtures: `readFullDocument` -> `writeOdt` -> the
//     parts of the package (decompressed) -> `readOdt` of that output (the model, by the canonical digest);
//   - the synthetic docx of R6 and R5 (tests/parity/sources.inputs, full.inputs) the same way;
//   - the generated `.odt` packages of js/quirk-odf.mjs (committed as tests/parity/odf.inputs): `readOdt`
//     (model digest or the class of the error) and `isOdt`;
//   - the strings of tests/parity/odf-units.inputs.txt through `lengthToTwips`, `lengthToHalfPt` and
//     `lengthToTwipsOr` (js/quirk-odf.mjs LENGTHS plus seeded strings).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-odf.mjs [--only <key prefix>]
//
// Writes tests/parity/odf.capture, one record per key; facets:
//   `<docx>#odt`        docx -> writeOdt: `rderr` (readFullDocument threw), `werr` (writeOdt threw), `names` (digest of
//                       the part names in order), `n`, `p<i>` (`sha16(name):sha32(bytes)` of each decompressed part),
//                       `isodt`, `rd` (readOdt of the output: `items:digest` of the model, or `err:<class>`);
//   `<docx>#odt-omit`   the same with `{ images: 'omit' }`, for the documents whose output has pictures: `names`, `n`, `p<i>`;
//   `quirk/odf/<name>.odt`  `rd`, `isodt`;
//   `unit/<i>`          `t`, `h`, `o`: lengthToTwips, lengthToHalfPt, lengthToTwipsOr(.., 7) as f64 bits (`u` = undefined).
// The model digest is the canonical stream (js/canon.mjs) of `{ blocks, sections, comments, styles }` of the document readOdt
// returns, with each cell given its `blocks` (the Rust cell is one story: paragraphs, then tables) and the empty `headers`/
// `footers` of a section left out.
import { Buffer } from 'node:buffer';
import { createHash } from 'node:crypto';
import { writeFileSync, existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { CaptureWriter, Stream, Rng, importFrom } from '../../uo-testkit/js/testkit.mjs';
import { crate, corpus, readBytes, ts, root, R2Canon } from './capture-r2-common.mjs';
import { quirkSources, QUIRK_SEED, QUIRK_COUNT } from './quirk-sources.mjs';
import { quirkFull } from './quirk-full.mjs';
import { quirkOdf, inputsText, LENGTHS } from './quirk-odf.mjs';
import { digestOdt } from './odf-canon.mjs';

const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : undefined;
const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const odf = await ts('odf/index.js');
const fflate = await importFrom(root, 'write/node_modules/fflate/lib/index.cjs');

const sha = (u8, n = 32) => createHash('sha256').update(u8).digest('hex').slice(0, n);
const shaText = (s, n = 16) => createHash('sha256').update(Buffer.from(s, 'utf8')).digest('hex').slice(0, n);
const bitsOf = (x) => { const b = Buffer.alloc(8); b.writeDoubleLE(Number.isNaN(x) ? NaN : x); return b.toString('hex'); };

/** The class of a failure of the TS (by its message). */
function classOf(e) {
  const m = String(e && e.message);
  if (m.includes('ZIP corrompido')) return 'zip';
  if (m.includes('sem content.xml')) return 'no-content';
  if (m.includes('sem office:body')) return 'no-text';
  if (m.includes('XML inválido')) return 'xml';
  if (e && e.name === 'RangeError' && /code point/i.test(m)) return 'xml';
  if (e && (e.name === 'InvalidCharacterError' || /not correctly encoded|Invalid character/.test(m))) return 'base64';
  if (e && e.name === 'RangeError') return 'limit';
  return `other:${e && e.name}:${m.slice(0, 60)}`;
}

function readOdtFacet(bytes) {
  try {
    return `ok:${digestOdt(odf.readOdt(bytes))}`;
  } catch (e) {
    return `err:${classOf(e)}`;
  }
}

const out = new CaptureWriter([
  'D5b ODF: writeOdt parts (decompressed) and readOdt model digests (docx-core odf/, frozen TS)',
  'facets: see js/capture-odf.mjs',
]);
let nrec = 0;
const want = (key) => !only || key.startsWith(only);

/** One docx (the package built by `make()`): writeOdt, the parts, readOdt of them. */
function recordDocx(key, make) {
  if (!want(`${key}#odt`)) return;
  const k = `${key}#odt`;
  let full;
  try {
    full = readFullDocument(make());
  } catch (e) {
    out.set(k, 'rderr', 'err');
    nrec++;
    return;
  }
  let bytes;
  try {
    bytes = odf.writeOdt(full);
  } catch (e) {
    out.set(k, 'werr', `err:${classOf(e)}`);
    nrec++;
    return;
  }
  const parts = Object.entries(fflate.unzipSync(bytes));
  out.set(k, 'names', shaText(parts.map(([n]) => n).join('\n')));
  out.set(k, 'n', String(parts.length));
  parts.forEach(([n, d], i) => out.set(k, `p${i}`, `${shaText(n)}:${sha(d)}`));
  out.set(k, 'isodt', odf.isOdt(bytes) ? '1' : '0');
  out.set(k, 'rd', readOdtFacet(bytes));
  nrec++;
  if (parts.some(([n]) => n.startsWith('Pictures/'))) {
    const k2 = `${key}#odt-omit`;
    const ob = odf.writeOdt(full, { images: 'omit' });
    const op = Object.entries(fflate.unzipSync(ob));
    out.set(k2, 'names', shaText(op.map(([n]) => n).join('\n')));
    out.set(k2, 'n', String(op.length));
    op.forEach(([n, d], i) => out.set(k2, `p${i}`, `${shaText(n)}:${sha(d)}`));
    nrec++;
  }
}

for (const { rel, file } of corpus()) {
  const bytes = readBytes(file);
  recordDocx(rel, () => DocxPackage.load(bytes));
}
const fromParts = (parts) => {
  const pkg = DocxPackage.empty();
  if (!parts.some(([path]) => path === 'word/document.xml')) pkg.removePart('word/document.xml');
  for (const [path, data] of parts) pkg.setPartText(path, data);
  return pkg;
};
// `nest-*` deeper than 16: the TS signatures are exponential there (as in capture-write-roundtrip.mjs); the writer
// here does not use them, but the reader's inputs are the same files, so the same ones are left out
const deep = (key) => { const m = /\/nest-[a-z]+-(\d+)\.docx$/.exec(key); return !!m && Number(m[1]) > 16; };
for (const { key, parts } of [...quirkSources(QUIRK_SEED, QUIRK_COUNT), ...quirkFull()]) if (!deep(key)) recordDocx(key, () => fromParts(parts));

// ───── generated .odt packages ─────
const odts = quirkOdf();
/** The bytes of a case: the zip of its parts (`mimetype` first, stored), with the damage it asks for. */
function zipOf(c) {
  if (c.raw) return c.raw;
  const files = {};
  for (const [name, data] of c.parts) files[name] = typeof data === 'string' ? new TextEncoder().encode(data) : data;
  let bytes = odf.writeZip(files);
  if (c.cut) bytes = bytes.slice(0, bytes.length - c.cut);
  if (c.cutTo) bytes = bytes.slice(0, Math.floor(bytes.length * c.cutTo));
  if (c.flip) {
    // flip one byte inside the stored/deflated data of the entry: the local header gives where it starts
    const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const at = (() => {
      let p = 0;
      while (p + 30 <= bytes.length && dv.getUint32(p, true) === 0x04034b50) {
        const csize = dv.getUint32(p + 18, true), nlen = dv.getUint16(p + 26, true), xlen = dv.getUint16(p + 28, true);
        const name = new TextDecoder().decode(bytes.subarray(p + 30, p + 30 + nlen));
        const start = p + 30 + nlen + xlen;
        if (name === c.flip) return start + Math.floor(csize / 2);
        p = start + csize;
      }
      return -1;
    })();
    if (at >= 0) bytes = Uint8Array.from(bytes, (b, i) => (i === at ? b ^ 0x5a : b));
  }
  return bytes;
}
for (const c of odts) {
  if (!want(c.key)) continue;
  const bytes = zipOf(c);
  out.set(c.key, 'rd', readOdtFacet(bytes));
  out.set(c.key, 'isodt', odf.isOdt(bytes) ? '1' : '0');
  nrec++;
}
if (!only) writeFileSync(join(crate, 'tests', 'parity', 'odf.inputs'), inputsText(odts, zipOf));

// ───── units ─────
const rng = new Rng(0x0df5b0a1);
const UNIT_ALPHABET = ['0', '1', '2', '3', '5', '7', '9', '.', '.', '-', '+', 'e', 'E', ' ', '\t', ' ', '﻿', 'c', 'm', 'i', 'n', 'p', 't', 'x', 'c', 'm', 'm', 'x', 'N', 'a', 'I', '٣'];
const UNITS = ['in', 'cm', 'mm', 'pt', 'pc', 'px', 'em', 'IN', 'Cm'];
const unitStrings = [...LENGTHS];
for (let i = 0; i < 2500; i++) {
  const kind = rng.below(4);
  if (kind === 0) unitStrings.push(Array.from({ length: 1 + rng.below(10) }, () => UNIT_ALPHABET[rng.below(UNIT_ALPHABET.length)]).join(''));
  else if (kind === 1) unitStrings.push(`${rng.below(3) ? '-' : ''}${rng.below(100000) / (rng.below(2) ? 1 : 10 ** rng.below(6))}${UNITS[rng.below(UNITS.length)]}`);
  else if (kind === 2) unitStrings.push(`${' \t'.slice(0, rng.below(3))}${rng.below(1000)}.${rng.below(1000)}${'  '.slice(0, rng.below(3))}${UNITS[rng.below(6)]}${' \n'.slice(0, rng.below(3))}`);
  else unitStrings.push(`${rng.below(10 ** 9) * rng.below(10 ** 9)}${UNITS[rng.below(6)]}`);
}
const escLine = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
if (!only) writeFileSync(join(crate, 'tests', 'parity', 'odf-units.inputs.txt'), `${unitStrings.map(escLine).join('\n')}\n`);
const fb = (x) => (x === undefined ? 'u' : bitsOf(x));
unitStrings.forEach((s, i) => {
  if (!want(`unit/${i}`)) return;
  const k = `unit/${i}`;
  out.set(k, 't', fb(odf.lengthToTwips(s)));
  out.set(k, 'h', fb(odf.lengthToHalfPt(s)));
  out.set(k, 'o', fb(odf.lengthToTwipsOr(s, 7)));
  nrec++;
});
// `undefined` inputs
out.set('unit/undefined', 't', fb(odf.lengthToTwips(undefined)));
out.set('unit/undefined', 'h', fb(odf.lengthToHalfPt(undefined)));
out.set('unit/undefined', 'o', fb(odf.lengthToTwipsOr(undefined, 7)));

const path = join(crate, 'tests', 'parity', only ? `odf.${only.replace(/[^a-z0-9]/gi, '_')}.capture` : 'odf.capture');
out.write(path);
console.log(`odf: ${nrec} records -> ${path}`);
