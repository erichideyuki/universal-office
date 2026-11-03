// Parity capture of the D1b font-embedding step (from uo-fonts G2): the frozen TS
// `readDocxEmbedding` and `writeDocxEmbedding` (shared/fonts/src/ooxml.ts) over
//   - the corpus docx (the oracle documents of docx-layout and the docx-core fixtures), and
//   - seeded synthetic packages that stress the edits (odd settings, font tables, relationships
//     and content types: self-closing roots, uppercase tags, `$` in attributes and names, entity
//     references, case-folded family names, missing and dangling parts, external targets),
// each with a seeded embedding plan of OPEN fonts only (shared/fonts/files/*.ttf; no proprietary
// font is read or written) and seeded settings, `crypto.getRandomValues` replaced by the testkit
// xorshift32 (one seed per record, 16 bytes per GUID, the low byte of each draw).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-font-embedding.mjs [--dump <key>]
//
// `--dump <key>` (a record key) writes the text of every part the step set for that record to
// `UO_PARITY_TRACE_DIR` (default `.`) as `ts-<part name with / as _>`; the Rust replay writes
// `rust-…` the same way (`UO_PARITY_TRACE=<key>`).
//
// Writes tests/parity/font-embedding.inputs (the synthetic packages as `@input <key>` + `<path>\t<escaped
// text | hex:bytes>`, and every record's plan as `@plan\t<key>\t<embed><subset><skip>\t<seed>` followed
// by `face\t<family>\t<variant>\t<font file>\t<0|1 subsetted>\t<panose>` lines) and
// tests/parity/font-embedding.capture, one record per `<docx>#fonts` with the facets
//   `before`   readDocxEmbedding before the step: JSON [embed, subset|null, skip|null, [families]]
//   `error`    the class of the exception when the step throws (then nothing else)
//   `order`    digest of the part names after the step, in package order
//   `set`      digest of the names of the parts the step set (setPart/setPartText), in package order
//   `parts`    digest of every set part: an .odttf part as its length, its first 32 bytes (the
//              obfuscated ones) and the plan face whose bytes its tail equals; any other part by its
//              bytes (decompressed)
//   `settings` `fontTable` `ftRels` `docRels` `types`  digest of that part's bytes after the step (or `-`)
//   `after`    readDocxEmbedding after the step
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { CaptureWriter, Rng, Stream, importFrom } from '../../uo-testkit/js/testkit.mjs';
import { corpus, crate, readBytes, repo, root, ts } from './capture-r2-common.mjs';

const { DocxPackage } = await ts('package.js');
const { readDocxEmbedding, writeDocxEmbedding } = await importFrom(root, 'shared/fonts/dist/ooxml.js');
const { faceInfo } = await importFrom(root, 'shared/fonts/dist/sfnt.js');

export const QUIRK_SEED = 0xf0e5ed01;
export const QUIRK_COUNT = 500;
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n';
const VARIANTS = ['regular', 'bold', 'italic', 'boldItalic'];

const fontDir = join(repo, 'shared', 'fonts', 'files');
const FONT_FILES = readdirSync(fontDir).filter((f) => f.endsWith('.ttf')).sort();
const FONTS = new Map(FONT_FILES.map((f) => [f, new Uint8Array(readFileSync(join(fontDir, f)))]));
const PANOSE = new Map(FONT_FILES.map((f) => [f, faceInfo(FONTS.get(f)).panose]));

/** Family names the plans draw from besides the document's own (escapes, `$`, case folding, non-BMP). */
const POOL = ['Calibri', 'Liberation Serif', 'A&B "q" <x>', '$&', '$`x$\'', 'Überschrift', 'ÜBERSCHRIFT', 'ſans', 'Kelvin K', 'Kelvin k',
  '日本語フォント', 'Emoji 😀', 'Wfit Test', 'times new roman', 'x.y*(z)', '', ' sp ace ', 'Σίσυφος', 'ΣΊΣΥΦΟΣ', 'İstanbul', 'ıstanbul'];

const esc = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
const pick = (rng, a) => a[rng.below(a.length)];

// ───────────────────────── synthetic packages ─────────────────────────

function contentTypes(rng) {
  const head = pick(rng, ['<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">', '<Types>', '<Types\txmlns="x" >']);
  const kids = [
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>',
    '<Default Extension="xml" ContentType="application/xml"/>',
    '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>',
  ];
  if (rng.below(4) === 0) kids.push(pick(rng, ['<Default Extension="odttf" ContentType="application/vnd.openxmlformats-officedocument.obfuscatedFont"/>',
    '<DEFAULT EXTENSION="ODTTF" CONTENTTYPE="x"/>', '<Default Extension="odttfx" ContentType="x"/>', '<Default a=">" Extension="odttf"/>']));
  if (rng.below(3) === 0) kids.push('<Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>');
  if (rng.below(3) === 0) kids.push('<Override PartName="/word/fontTable.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml"/>');
  switch (rng.below(12)) {
    case 0: return undefined;
    case 1: return '';
    case 2: return DECL + head.replace('>', '/>').replace('//>', '/>');
    case 3: return DECL + head + kids.join('') + '</Types>\r\n  ';
    case 4: return DECL + head + kids.join('');
    default: return DECL + head + kids.join('') + '</Types>';
  }
}

function docRels(rng) {
  const rels = [];
  const target = (name) => pick(rng, [name, name, name, `/word/${name}`, `./${name}`, `missing-${name}`, `../word/${name}`, name.toUpperCase(), `${name}#x`]);
  if (rng.below(5)) rels.push(`<Relationship Id="rId${1 + rng.below(4)}" Type="${REL}/settings" Target="${target('settings.xml')}"${rng.below(15) ? '' : ' TargetMode="External"'}/>`);
  if (rng.below(5)) rels.push(`<Relationship Id="rId${1 + rng.below(6)}" Type="${REL}/fontTable" Target="${target('fontTable.xml')}"${rng.below(15) ? '' : ' TargetMode="External"'}/>`);
  if (rng.below(2)) rels.push(`<Relationship Id="rId${1 + rng.below(8)}" Type="${REL}/styles" Target="styles.xml"/>`);
  if (rng.below(6) === 0) rels.push('<Relationship Type="t" Target="x"/>');
  for (let i = rels.length - 1; i > 0; i--) { const j = rng.below(i + 1); [rels[i], rels[j]] = [rels[j], rels[i]]; }
  switch (rng.below(10)) {
    case 0: return undefined;
    case 1: return DECL + '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>';
    case 2: return DECL + '<Relationships xmlns="x">' + rels.join('\n') + '</Relationships>\n';
    default: return DECL + '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' + rels.join('') + '</Relationships>';
  }
}

function settings(rng) {
  const flag = (n) => pick(rng, [`<w:${n}/>`, `<w:${n} w:val="0"/>`, `<w:${n} w:val="false"/>`, `<w:${n} w:val="off"></w:${n}>`, `<w:${n} w:val="1"/>`,
    `<w:${n}></w:${n}>`, `<w:${n} />`, `<w:${n}\n/>`, `<w:${n}X/>`, `<w:${n}>`]);
  const kids = [];
  for (let n = rng.below(6); n > 0; n--) {
    kids.push(pick(rng, [flag('embedTrueTypeFonts'), flag('embedSystemFonts'), flag('saveSubsetFonts'), '<w:zoom w:percent="100"/>', '<w:view w:val="web"/>',
      '<w:writeProtection w:recommended="1"/>', '<w:printFormsData/>', '<w:defaultTabStop w:val="720"/>', '<w:compat><w:compatSetting w:name="x" w:val="15"/></w:compat>',
      '<w:removePersonalInformation></w:removePersonalInformation>', '<!-- <w:zoom/> -->', ' ', '\n', 'é']));
  }
  const attrs = pick(rng, [` xmlns:w="${W}"`, ` xmlns:w="${W}" mc:Ignorable="w14"`, ' a="$&"', ' a="$`$\'"', '', ` xmlns:w="${W}"  `]);
  switch (rng.below(14)) {
    case 0: return undefined;
    case 1: return '';
    case 2: return DECL + `<w:settings${attrs}/>`;
    case 3: return DECL + `<w:settings${attrs} />` + kids.join('');
    case 4: return DECL + `<s:settings xmlns:s="${W}">` + kids.join('') + '</s:settings>';
    case 5: return DECL + `<w:settingsX/><w:settings${attrs}>` + kids.join('') + '</w:settings>';
    default: return DECL + `<w:settings${attrs}>` + kids.join('') + '</w:settings>';
  }
}

const NAMES = ['Calibri', 'Cambria', 'A&amp;B &quot;q&quot; &lt;x&gt;', '$&amp;', 'Überschrift', 'überschrift', 'ſans', 'Kelvin K', 'times New Roman',
  'Wfit Test', 'Emoji 😀', '', 'Σίσυφος', 'İstanbul', 'Liberation Serif'];

function fontTable(rng) {
  const kids = [];
  for (let n = rng.below(5); n > 0; n--) {
    const name = pick(rng, NAMES);
    const embed = rng.below(3) === 0 ? pick(rng, ['<w:embedRegular r:id="rId1" w:fontKey="{00000000-0000-0000-0000-000000000000}"/>', '<w:embedBold r:id="rId9"/>',
      '<w:embedItalic r:id="rId2"></w:embedItalic>', '<w:embedBoldItalic/>', '<w:embedBoldX/>']) : '';
    kids.push(pick(rng, [
      `<w:font w:name="${name}"><w:panose1 w:val="020F0502020204030204"/><w:charset w:val="00"/>${embed}</w:font>`,
      `<w:font w:name="${name}"/>`, `<w:font w:name="${name}" />`, `<w:font w:name="${name}">${embed}</W:FONT>`, `<W:FONT W:NAME="${name.toUpperCase()}">${embed}</w:font>`,
      `<w:font w:altName="x" w:name="${name}">${embed}</w:font>`, `<w:font w:name="${name}">${embed}`,
    ]));
  }
  const xmlnsR = rng.below(3) ? ` xmlns:r="${REL}"` : '';
  switch (rng.below(12)) {
    case 0: return undefined;
    case 1: return '';
    case 2: return DECL + `<w:fonts xmlns:w="${W}"${xmlnsR}/>`;
    case 3: return DECL + `<w:fonts xmlns:w="${W}"${xmlnsR} />\n`;
    case 4: return DECL + `<w:fonts xmlns:w="${W}"${xmlnsR}>` + kids.join('');
    case 5: return DECL + `<w:fonts xmlns:w="${W}"${xmlnsR}>` + kids.join('') + '</w:fonts>\r\n\t';
    default: return DECL + `<w:fonts xmlns:w="${W}"${xmlnsR}>` + kids.join('') + '</w:fonts>';
  }
}

function fontRels(rng) {
  const rels = [];
  for (let n = rng.below(4); n > 0; n--) {
    rels.push(`<Relationship Id="rId${1 + rng.below(5)}" Type="${pick(rng, [`${REL}/font`, `${REL}/font`, `${REL}/image`])}" Target="${pick(rng,
      ['fonts/font1.odttf', 'fonts/FONT2.ODTTF', 'fonts/font3.odttf', '../fonts/x.odttf', '/word/fonts/font1.odttf', 'https://x.test/f', 'fonts/font%31.odttf'])}"${rng.below(8) ? '' : ' TargetMode="External"'}/>`);
  }
  if (rng.below(4) === 0) return undefined;
  return DECL + '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' + rels.join('') + '</Relationships>';
}

function quirkPackages(seed, count) {
  const rng = new Rng(seed);
  const out = [];
  for (let i = 0; i < count; i++) {
    const parts = [];
    const add = (path, data) => { if (data !== undefined) parts.push([path, data]); };
    add('[Content_Types].xml', contentTypes(rng));
    add('_rels/.rels', DECL + `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${REL}/officeDocument" Target="word/document.xml"/></Relationships>`);
    add('word/document.xml', DECL + `<w:document xmlns:w="${W}"><w:body><w:p/></w:body></w:document>`);
    add('word/_rels/document.xml.rels', docRels(rng));
    add('word/settings.xml', settings(rng));
    add('word/fontTable.xml', fontTable(rng));
    add('word/_rels/fontTable.xml.rels', fontRels(rng));
    if (rng.below(2)) add('word/fonts/font1.odttf', new Uint8Array([1, 2, 3]));
    if (rng.below(3) === 0) add('word/fonts/FONT2.ODTTF', new Uint8Array([4, 5]));
    if (rng.below(4) === 0) add('word/fonts/font4.odttf', new Uint8Array([6]));
    if (rng.below(6) === 0) add('fonts/font1.odttf', new Uint8Array([7]));
    out.push({ key: `quirk/fonts/${seed.toString(16)}-${i}.docx`, parts });
  }
  return out;
}

// ───────────────────────── plans ─────────────────────────

const docFamilies = (pkg) => [...(pkg.getPartText('word/fontTable.xml') ?? '').matchAll(/w:name="([^"]*)"/g)].map((m) => m[1]
  .replace(/&quot;/g, '"').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&apos;/g, "'").replace(/&amp;/g, '&'));

function planOf(rng, pkg) {
  const own = docFamilies(pkg);
  const settings = { embed: rng.below(5) !== 0, subset: rng.below(2) === 0, skipCommonFonts: rng.below(2) === 0 };
  const faces = [];
  for (let n = rng.below(7); n > 0; n--) {
    let family = own.length && rng.below(2) ? pick(rng, own) : pick(rng, POOL);
    if (rng.below(4) === 0) family = rng.below(2) ? family.toUpperCase() : family.toLowerCase();
    const file = pick(rng, FONT_FILES);
    faces.push({ family, variant: pick(rng, VARIANTS), file, subsetted: rng.below(3) === 0, panose: PANOSE.get(file) });
  }
  return { settings, faces, seed: (rng.next() >>> 0) || 1 };
}

// ───────────────────────── capture ─────────────────────────

function readFacet(pkg) {
  const r = readDocxEmbedding(pkg);
  return JSON.stringify([r.settings.embed, r.settings.subset ?? null, r.settings.skipCommonFonts ?? null, [...r.embedded]]);
}

const digestText = (s) => new Stream().str(s).finish().short;
const partDigest = (data) => (data === undefined ? '-' : new Stream().digestOf(data).finish().short);

const argv = process.argv.slice(2);
const dumpKey = argv.includes('--dump') ? argv[argv.indexOf('--dump') + 1] : undefined;
const traceDir = process.env.UO_PARITY_TRACE_DIR ?? '.';

function facets(pkg, plan, key) {
  const out = { before: readFacet(pkg) };
  const written = new Set();
  const setPart = pkg.setPart.bind(pkg);
  const setPartText = pkg.setPartText.bind(pkg);
  pkg.setPart = (p, d) => { written.add(p); setPart(p, d); };
  pkg.setPartText = (p, t) => { written.add(p); setPartText(p, t); };
  const rng = new Rng(plan.seed);
  globalThis.crypto.getRandomValues = (b) => { for (let i = 0; i < b.length; i++) b[i] = rng.next() & 0xff; return b; };
  const faces = plan.faces.map((f) => ({ family: f.family, variant: f.variant, bytes: FONTS.get(f.file), subsetted: f.subsetted, info: { panose: f.panose } }));
  try {
    writeDocxEmbedding(pkg, { faces, skipped: [] }, plan.settings);
  } catch (e) {
    return { before: out.before, error: e?.constructor?.name ?? 'Error' };
  }
  const names = pkg.partNames();
  out.order = digestText(JSON.stringify(names));
  const set = names.filter((n) => written.has(n));
  if (key === dumpKey) {
    for (const n of set) if (!n.toLowerCase().endsWith('.odttf')) writeFileSync(join(traceDir, `ts-${n.replace(/[\/\[\]]/g, '_')}`), pkg.getPart(n));
    writeFileSync(join(traceDir, 'ts-names.json'), JSON.stringify({ names, set }, null, 1));
  }
  out.set = digestText(JSON.stringify(set));
  const st = new Stream();
  for (const n of set) {
    const data = pkg.getPart(n);
    st.str(n);
    if (n.toLowerCase().endsWith('.odttf')) {
      const src = plan.faces.find((f) => { const b = FONTS.get(f.file); return b.length === data.length && Buffer.compare(b.subarray(32), data.subarray(32)) === 0; });
      st.u64(data.length).bytes(data.subarray(0, 32)).str(src ? src.file : '?');
      if (!src) st.digestOf(data);
    } else {
      st.digestOf(data);
    }
  }
  out.parts = st.finish().short;
  out.settings = partDigest(pkg.getPart('word/settings.xml'));
  out.fontTable = partDigest(pkg.getPart('word/fontTable.xml'));
  out.ftRels = partDigest(pkg.getPart('word/_rels/fontTable.xml.rels'));
  out.docRels = partDigest(pkg.getPart('word/_rels/document.xml.rels'));
  out.types = partDigest(pkg.getPart('[Content_Types].xml'));
  out.after = readFacet(pkg);
  return out;
}

const cap = new CaptureWriter(['D1b font embedding (from uo-fonts G2): the frozen TS readDocxEmbedding/writeDocxEmbedding, see js/capture-font-embedding.mjs']);
const lines = ['# uo-docx D1b font embedding inputs (js/capture-font-embedding.mjs): synthetic packages (`@input <key>`, `<path>\\t<escaped text | hex:bytes>`) and every record\'s plan (`@plan\\t<key>\\t<embed><subset><skip>\\t<guid seed>`, then `face\\t<family>\\t<variant>\\t<font file>\\t<subsetted>\\t<panose>`)'];
const planLines = [];
let docs = 0, skipped = 0, errors = 0, faceCount = 0;
const prng = new Rng(0xf0e5ed02);

function record(key, pkg) {
  const plan = planOf(prng, pkg);
  const s = plan.settings;
  planLines.push(`@plan\t${key}\t${+s.embed}${+s.subset}${+s.skipCommonFonts}\t${plan.seed}`);
  for (const f of plan.faces) planLines.push(`face\t${esc(f.family)}\t${f.variant}\t${f.file}\t${+f.subsetted}\t${esc(f.panose)}`);
  faceCount += plan.faces.length;
  const out = facets(pkg, plan, key);
  for (const [facet, value] of Object.entries(out)) cap.set(key, facet, value);
  if (out.error) errors++;
  docs++;
}

for (const { rel, file } of corpus()) {
  let pkg;
  try { pkg = DocxPackage.load(readBytes(file)); } catch { skipped++; continue; }
  record(`${rel}#fonts`, pkg);
}
const quirks = quirkPackages(QUIRK_SEED, QUIRK_COUNT);
for (const { key, parts } of quirks) {
  const pkg = DocxPackage.empty();
  pkg.parts = new Map();
  for (const [path, data] of parts) {
    if (typeof data === 'string') pkg.setPartText(path, data); else pkg.setPart(path, data);
  }
  lines.push(`@input ${key}`);
  for (const [path, data] of parts) lines.push(`${path}\t${typeof data === 'string' ? esc(data) : 'hex:' + Buffer.from(data).toString('hex')}`);
  record(`${key}#fonts`, pkg);
}

writeFileSync(join(crate, 'tests', 'parity', 'font-embedding.inputs'), [...lines, ...planLines].join('\n') + '\n');
cap.meta('corpus', `docx=${docs - QUIRK_COUNT} skipped=${skipped} quirk=${QUIRK_COUNT} faces=${faceCount} errors=${errors} fonts=${FONT_FILES.join(',')}`);
cap.write(join(crate, 'tests', 'parity', 'font-embedding.capture'));
console.log(`records ${docs} (skipped ${skipped}), faces ${faceCount}, errors ${errors}`);
