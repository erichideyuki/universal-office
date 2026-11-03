// Capture of frontier R4a (props mapping, facet `props-map`): the frozen TS `runPropsFromRPr`,
// `editBaseProps`, `paraPropsFromPPr`, `readSectPr` and `readBorderEdge` over every `w:rPr`,
// `w:pPr`, `w:sectPr` and paragraph-border edge of the corpus, in canonical streams (A6).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-props-map.mjs [--trace <key>]
//
// Corpus: every git-tracked .docx of the repository (read from this worktree) plus a seeded
// generated attribute-quirk corpus (`tests/parity/props-map.quirks.txt`, one XML part per line,
// replayed by the Rust test as it is). One record per part: the elements are walked in document
// order; per element the stream holds `E`, its ordinal and the digest input of its kind:
//   w:rPr    runPropsFromRPr(readRPr(el)), then editBaseProps(run, readRPr(el)) for four runs
//            (a link, a note reference, a symbol font, all three)
//   w:pPr    paraPropsFromPPr(readPPr(el))
//   w:sectPr readSectPr(el)
//   w:top|left|bottom|right|between|bar under w:pBdr: readBorderEdge(el)
// Facets: `n` (elements walked), `props-map` (the values) and `order` (the key-order facet, A6).
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Stream, CaptureWriter, importFrom, tsRoot, Rng } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..', '..', '..');
const root = tsRoot();
const { DocxPackage } = await importFrom(root, 'write/packages/docx-core/dist/package.js');
const { parseXml } = await importFrom(root, 'write/packages/docx-core/dist/xml.js');
const { readRPr, readPPr } = await importFrom(root, 'write/packages/docx-core/dist/ooxml/props.js');
const { runPropsFromRPr, editBaseProps, paraPropsFromPPr, readSectPr, readBorderEdge } = await importFrom(root, 'write/packages/docx-core/dist/document.js');

const traceKey = process.argv.includes('--trace') ? process.argv[process.argv.indexOf('--trace') + 1] : undefined;
const PART = /^word\/[^/]+\.xml$/;
const EDGES = new Set(['w:top', 'w:left', 'w:bottom', 'w:right', 'w:between', 'w:bar']);

const BASE_RUNS = [
  { text: 'x', props: { link: 'https://x' } },
  { text: 'x', props: {}, noteRef: { kind: 'footnote', id: 1 } },
  { text: 'x', props: {}, symFont: 'Wingdings' },
  { text: 'x', props: { link: 'https://x' }, noteRef: { kind: 'footnote', id: 1 }, symFont: 'Wingdings' },
];

/** Elements in document order (pre-order), iterative. */
function* elements(rootEl) {
  const stack = [[rootEl, null]];
  while (stack.length) {
    const [el, parent] = stack.pop();
    yield [el, parent];
    for (let i = el.children.length - 1; i >= 0; i--) if (typeof el.children[i] !== 'string') stack.push([el.children[i], el]);
  }
}

function digestPart(xmlText, key) {
  const root = parseXml(xmlText);
  const traced = traceKey === key;
  const s = new Stream({ trace: traced });
  const o = new Stream({ trace: traced });
  const canon = new Canon(s, { order: o });
  let n = 0;
  for (const [el, parent] of elements(root)) {
    let kind;
    if (el.name === 'w:rPr' || el.name === 'w:pPr' || el.name === 'w:sectPr') kind = el.name;
    else if (EDGES.has(el.name) && parent?.name === 'w:pBdr') kind = 'edge';
    else continue;
    s.tag('E').u32(n);
    o.tag('E').u32(n);
    n++;
    if (kind === 'w:rPr') {
      const rpr = readRPr(el);
      canon.value(runPropsFromRPr(rpr), 'runProps');
      for (const run of BASE_RUNS) canon.value(editBaseProps(run, rpr), 'runProps');
    } else if (kind === 'w:pPr') canon.value(paraPropsFromPPr(readPPr(el)), 'paraProps');
    else if (kind === 'w:sectPr') canon.value(readSectPr(el), 'sectPr');
    else canon.value(readBorderEdge(el), 'edge');
  }
  if (traced) { writeFileSync(`${key.replace(/[^\w.-]+/g, '_')}.ts.trace`, s.trace()); writeFileSync(`${key.replace(/[^\w.-]+/g, '_')}.ts.order.trace`, o.trace()); }
  return { n, main: s.finish().short, order: o.finish().short };
}

// ---- the generated attribute-quirk corpus (seeded) ----------------------------------------
const rng = new Rng(0x4a4a0001);
const pick = (a) => a[rng.below(a.length)];
const chance = (k) => rng.below(k) === 0;
const NUM = ['0', '1', '2', '3', '-1', '-0', '12', '100', '240', '1002', '1701', '11906', '12pt', '1e3', ' 12 ', '0x10', 'NaN', 'Infinity', '', 'abc', '1.5', '+7', '99999999999999999999'];
const BIG = '9'.repeat(310);
const pickNum = () => (rng.below(1500) === 0 ? BIG : pickNum());
const BOOL = ['1', '0', 'true', 'false', 'on', 'off', '', 'x'];
const WORD = ['single', 'none', 'nil', 'double', 'dotted', 'wave', 'words', 'dash', 'auto', '', 'x', 'thick', 'wavyDouble', 'dashDotDotHeavy'];
const HEX = ['FF0000', '0563C1', 'auto', '', '00ff00', 'GGGGGG', '4472C4'];
const attrs = (o) => Object.entries(o).filter(([, v]) => v !== undefined).map(([k, v]) => ` ${k}="${v}"`).join('');
const opt = (v, k = 3) => (chance(k) ? undefined : v);
const el = (name, a = {}, inner = '') => (inner === '' ? `<${name}${attrs(a)}/>` : `<${name}${attrs(a)}>${inner}</${name}>`);

function genRPr(depth = 0, inner = '') {
  const kids = [];
  if (chance(2)) kids.push(el('w:rStyle', { 'w:val': pick(['Strong', '', 'x']) }));
  if (chance(2)) kids.push(el('w:rFonts', { 'w:ascii': opt(pick(['Aptos', '', 'Times'])), 'w:hAnsi': opt('H'), 'w:eastAsia': opt('E'), 'w:asciiTheme': opt('minorHAnsi') }));
  for (const t of ['w:b', 'w:i', 'w:strike', 'w:dstrike', 'w:caps', 'w:smallCaps', 'w:vanish']) if (chance(3)) kids.push(el(t, { 'w:val': opt(pick(BOOL)) }));
  if (chance(2)) kids.push(el('w:u', { 'w:val': pick(WORD), 'w:color': opt(pick(HEX)), 'w:themeColor': opt(pick(['accent1', '', 'x'])), 'w:themeTint': opt(pick(['99', ''])), 'w:themeShade': opt(pick(['80', ''])) }));
  if (chance(2)) kids.push(el('w:color', { 'w:val': opt(pick(HEX)), 'w:themeColor': opt(pick(['accent1', '', 'text1'])), 'w:themeTint': opt(pick(['99', ''])), 'w:themeShade': opt(pick(['80', ''])) }));
  for (const t of ['w:w', 'w:position', 'w:kern', 'w:sz', 'w:spacing']) if (chance(2)) kids.push(el(t, { 'w:val': opt(pickNum()) }));
  if (chance(2)) kids.push(el('w:vertAlign', { 'w:val': pick(['superscript', 'subscript', 'baseline', '', 'x']) }));
  if (chance(3)) kids.push(el('w:highlight', { 'w:val': pick(['yellow', 'none', '', 'x']) }));
  if (chance(4)) kids.push(el('w14:ligatures', { 'w14:val': pick(['standard', 'none', 'x']) }));
  if (chance(4)) kids.push(el('w14:numForm', { 'w14:val': pick(['oldStyle', 'lining', 'x']) }));
  if (chance(5)) kids.push(el('w14:textFill', {}, el('w14:solidFill', {}, el('w14:srgbClr', { 'w14:val': pick(HEX) }))));
  if (inner) kids.push(inner);
  else if (depth < 3 && chance(6)) kids.push(el('w:rPrChange', { 'w:author': pick(['a', '', 'b']), 'w:date': opt('2024-01-01T00:00:00Z'), 'w16du:dateUtc': opt('2024-01-01T00:00:00Z') }, genRPr(depth + 1)));
  // shuffle: the key order is data-dependent
  for (let i = kids.length - 1; i > 0; i--) { const j = rng.below(i + 1); [kids[i], kids[j]] = [kids[j], kids[i]]; }
  return el('w:rPr', {}, kids.join(''));
}

function genBorder(tag) {
  return el(tag, { 'w:val': opt(pick(WORD)), 'w:sz': opt(pickNum()), 'w:space': opt(pickNum()), 'w:color': opt(pick(HEX)), 'w:themeColor': opt('accent1'), 'w:shadow': opt(pick(BOOL), 6), 'w:frame': opt(pick(BOOL), 6) });
}

function genPPr(depth = 0, inner = '') {
  const kids = [];
  if (chance(2)) kids.push(el('w:jc', { 'w:val': pick(['left', 'center', 'right', 'both', 'start', 'end', 'distribute', 'x', '']) }));
  if (chance(3)) kids.push(el('w:pStyle', { 'w:val': pick(['Heading1', '', 'x']) }));
  if (chance(3)) kids.push(el('w:numPr', {}, (chance(2) ? el('w:ilvl', { 'w:val': pickNum() }) : '') + (chance(2) ? el('w:numId', { 'w:val': pick(['1002', '0', '5', ...NUM]) }) : '')));
  if (chance(2)) kids.push(el('w:ind', { 'w:left': opt(pickNum()), 'w:right': opt(pickNum()), 'w:firstLine': opt(pickNum()), 'w:hanging': opt(pickNum()) }));
  if (chance(2)) kids.push(el('w:spacing', { 'w:line': opt(pickNum()), 'w:lineRule': opt(pick(['auto', 'exact', 'atLeast', ''])), 'w:before': opt(pickNum()), 'w:after': opt(pickNum()), 'w:beforeAutospacing': opt(pick(BOOL), 6), 'w:afterAutospacing': opt(pick(BOOL), 6) }));
  if (chance(3)) kids.push(el('w:shd', { 'w:fill': opt(pick(HEX)), 'w:val': 'clear' }));
  if (chance(3)) kids.push(el('w:tabs', {}, Array.from({ length: rng.below(4) }, () => el('w:tab', { 'w:val': pick(['left', 'center', 'clear', 'bar', 'num', 'x']), 'w:pos': pickNum(), 'w:leader': opt(pick(['dot', 'none', '', 'underscore'])) })).join('')));
  if (chance(3)) kids.push(el('w:pBdr', {}, ['top', 'left', 'bottom', 'right', 'between', 'bar'].filter(() => chance(2)).map((s) => genBorder(`w:${s}`)).join('')));
  for (const t of ['w:keepNext', 'w:keepLines', 'w:pageBreakBefore', 'w:widowControl', 'w:suppressLineNumbers', 'w:suppressAutoHyphens']) if (chance(4)) kids.push(el(t, { 'w:val': opt(pick(BOOL)) }));
  if (chance(5)) kids.push(el('w:textboxTightWrap', { 'w:val': pick(['none', 'allLines', 'firstAndLastLine', 'x', '']) }));
  if (chance(4)) kids.push(el('w:outlineLvl', { 'w:val': opt(pickNum()) }));
  if (chance(4)) kids.push(el('w:framePr', { 'w:dropCap': opt(pick(['drop', 'margin', 'none', ''])) }));
  if (chance(4)) kids.push(el('w:rPr', {}, pick([el('w:ins', { 'w:author': 'a', 'w:date': '2024-01-01T00:00:00Z' }), el('w:del', { 'w:author': 'b' }), el('w:moveFrom', { 'w:author': 'c', 'w16du:dateUtc': '2024' }), el('w:moveTo', { 'w:author': 'd' })])));
  if (inner) kids.push(inner);
  else if (depth < 3 && chance(6)) kids.push(el('w:pPrChange', { 'w:author': pick(['a', '']), 'w:date': opt('2024-01-01T00:00:00Z') }, genPPr(depth + 1)));
  for (let i = kids.length - 1; i > 0; i--) { const j = rng.below(i + 1); [kids[i], kids[j]] = [kids[j], kids[i]]; }
  return el('w:pPr', {}, kids.join(''));
}

function genSectPr() {
  const kids = [];
  if (chance(2)) kids.push(el('w:pgSz', { 'w:w': opt(pickNum()), 'w:h': opt(pickNum()) }));
  if (chance(2)) kids.push(el('w:pgMar', { 'w:left': opt(pickNum()), 'w:right': opt(pickNum()), 'w:top': opt(pickNum()), 'w:bottom': opt(pickNum()), 'w:header': opt(pickNum()), 'w:footer': opt(pickNum()) }));
  if (chance(2)) kids.push(el('w:type', { 'w:val': pick(['continuous', 'evenPage', 'oddPage', 'nextColumn', 'nextPage', 'x', '']) }));
  if (chance(2)) {
    const num = rng.below(40) === 0 ? BIG : pick(['2', '3', '1', '0', '4', ' 2 ', '2x', 'abc', '-3', '']);
    const defs = Array.from({ length: rng.below(5) }, () => el('w:col', { 'w:w': opt(pickNum()), 'w:space': opt(pickNum()) })).join('');
    kids.push(el('w:cols', { 'w:num': opt(num), 'w:space': opt(pickNum()), 'w:sep': opt(pick(BOOL)), 'w:equalWidth': opt(pick(['0', 'false', '1', 'true', ''])) }, defs));
  }
  if (chance(3)) kids.push(el('w:titlePg', { 'w:val': opt(pick(BOOL)) }));
  if (chance(3)) kids.push(el('w:pgBorders', { 'w:offsetFrom': opt(pick(['page', 'text', 'x'])), 'w:display': opt(pick(['allPages', 'firstPage', 'notFirstPage', 'x'])), 'w:zOrder': opt(pick(['front', 'back', 'x'])) }, ['top', 'left', 'bottom', 'right'].filter(() => chance(2)).map((s) => genBorder(`w:${s}`)).join('')));
  if (chance(2)) kids.push(el('w:pgNumType', { 'w:start': opt(pickNum()), 'w:fmt': opt(pick(['lowerRoman', 'decimal', ''])), 'w:chapStyle': opt(pickNum()), 'w:chapSep': opt(pick(['period', 'colon', 'emDash', 'enDash', 'hyphen', 'x'])) }));
  for (const t of ['w:footnotePr', 'w:endnotePr']) if (chance(3)) kids.push(el(t, {}, (chance(2) ? el('w:pos', { 'w:val': pick(['pageBottom', 'beneathText', '']) }) : '') + (chance(2) ? el('w:numFmt', { 'w:val': pick(['decimal', '']) }) : '') + (chance(2) ? el('w:numStart', { 'w:val': pickNum() }) : '') + (chance(2) ? el('w:numRestart', { 'w:val': pick(['continuous', 'eachSect', 'eachPage', 'x']) }) : '')));
  if (chance(3)) kids.push(el('w15:footnoteColumns', { 'w:val': opt(pickNum()) }));
  if (chance(3)) kids.push(el('w:lnNumType', { 'w:countBy': opt(pickNum()), 'w:start': opt(pickNum()), 'w:distance': opt(pickNum()), 'w:restart': opt(pick(['newSection', 'continuous', 'newPage', 'x'])) }));
  if (chance(3)) kids.push(el('w:vAlign', { 'w:val': pick(['center', 'both', 'bottom', 'top', 'x', '']) }));
  for (let i = kids.length - 1; i > 0; i--) { const j = rng.below(i + 1); [kids[i], kids[j]] = [kids[j], kids[i]]; }
  return el('w:sectPr', {}, kids.join(''));
}

function deepChain(n) {
  let r = el('w:b', { 'w:val': '0' });
  let rpr = `<w:rPr>${r}</w:rPr>`;
  let ppr = `<w:pPr><w:keepNext/></w:pPr>`;
  for (let i = 0; i < n; i++) {
    rpr = `<w:rPr><w:i/><w:rPrChange w:author="a${i}">${rpr}</w:rPrChange></w:rPr>`;
    ppr = `<w:pPr><w:keepLines/><w:pPrChange w:author="p${i}">${ppr}</w:pPrChange></w:pPr>`;
  }
  return `<w:p>${ppr}<w:r>${rpr}</w:r></w:p>`;
}

const NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml" xmlns:w16du="http://schemas.microsoft.com/office/word/2023/wordml/word16du"';
const quirkParts = [];
for (let i = 0; i < 300; i++) {
  const body = [];
  const k = 1 + rng.below(3);
  for (let j = 0; j < k; j++) {
    body.push(`<w:p>${genPPr()}<w:r>${genRPr()}<w:t>t</w:t></w:r></w:p>`);
    if (chance(3)) body.push(genSectPr());
  }
  quirkParts.push(`<w:document ${NS}><w:body>${body.join('')}${genSectPr()}</w:body></w:document>`);
}
quirkParts.push(`<w:document ${NS}><w:body>${deepChain(100)}${deepChain(300)}</w:body></w:document>`);
quirkParts.push(`<w:document ${NS}><w:body><w:sectPr/><w:p><w:pPr/><w:r><w:rPr/></w:r></w:p></w:body></w:document>`);
mkdirSync(join(here, '..', 'tests', 'parity'), { recursive: true });
writeFileSync(join(here, '..', 'tests', 'parity', 'props-map.quirks.txt'), quirkParts.join('\n') + '\n');

// ---- capture --------------------------------------------------------------------------------
const cap = new CaptureWriter(['uo-docx R4a: props mapping (runPropsFromRPr, editBaseProps, paraPropsFromPPr, readSectPr, readBorderEdge) of the frozen TS; facets n, props-map, order']);
const files = execFileSync('git', ['ls-files', '*.docx'], { cwd: repo, encoding: 'utf8', maxBuffer: 1 << 26 }).split('\n').filter(Boolean).sort();
let parts = 0, elementsTotal = 0, skipped = 0;
const add = (key, r) => { cap.set(key, 'n', r.n); cap.set(key, 'props-map', r.main); cap.set(key, 'order', r.order); parts++; elementsTotal += r.n; };
for (const f of files) {
  let pkg;
  try { pkg = DocxPackage.load(new Uint8Array(readFileSync(join(repo, f)))); } catch { skipped++; continue; }
  for (const name of pkg.partNames().filter((p) => PART.test(p)).sort()) {
    const text = pkg.getPartText(name);
    if (text === undefined) continue;
    try { add(`${f}#${name}`, digestPart(text, `${f}#${name}`)); } catch { skipped++; }
  }
}
quirkParts.forEach((text, i) => add(`quirk#${i}`, digestPart(text, `quirk#${i}`)));
{
  const s = new Stream(); const o = new Stream();
  new Canon(s, { order: o }).value(readSectPr(undefined), 'sectPr');
  cap.set('synthetic#sectPr-undefined', 'n', 1); cap.set('synthetic#sectPr-undefined', 'props-map', s.finish().short); cap.set('synthetic#sectPr-undefined', 'order', o.finish().short);
}
mkdirSync(join(here, '..', 'tests', 'parity'), { recursive: true });
cap.write(join(here, '..', 'tests', 'parity', 'props-map.capture'));
console.log(`${files.length} docx, ${parts} parts, ${elementsTotal} elements, ${skipped} skipped`);
