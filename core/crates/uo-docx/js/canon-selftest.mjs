// Self-test of the canonical digest (A6): the TS walker (canon.mjs) over a small hand-built
// TS document (the shapes `readFullDocument` produces, with the aliased views, Maps, explicit
// `undefined` keys, NaN, -0, a lone surrogate, bytes, source spans) writes the digests the
// Rust test `canon_selftest` must reproduce from the equivalent hand-built Rust model.
//
//   node core/crates/uo-docx/js/canon-selftest.mjs [--trace <dir>]
//
// Pure JS (no checkout of main needed); the expected file is committed.
import { writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Stream } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const traceDir = process.argv.includes('--trace') ? process.argv[process.argv.indexOf('--trace') + 1] : undefined;

const DEF = () => ({ bold: false, italic: false, underline: false, strike: false, allCaps: false, smallCaps: false });

export function sampleDocument() {
  const run1 = { text: 'Olá 😀\uD800', props: { ...DEF(), bold: true, fontAscii: undefined, src: 2 }, rpr: { b: true, sz: NaN, rFonts: { ascii: 'Aptos' } } };
  const para1 = {
    runs: [run1],
    props: { styleId: 'Heading1', src: 1, blockSdt: { key: 's7', gallery: 'Table of Contents' } },
    ppr: { pStyle: 'Heading1', spacing: { before: -0 } },
  };
  const cellPara = { runs: [{ text: 'cell', props: DEF() }], props: {} };
  const cell = { paragraphs: [cellPara], tables: [], gridSpan: 1, widthTwips: undefined, shadingHex: undefined, borders: undefined, blocks: [{ type: 'paragraph', paragraph: cellPara }] };
  const table = { type: 'table', rows: [{ cells: [cell] }], gridColsTwips: [2000], borders: undefined };
  const tbPara = { runs: [], props: {} };
  const tbRun = { text: '￼', props: DEF(), textbox: { paras: [tbPara], widthTwips: 100, heightTwips: 50, blocks: [{ type: 'paragraph', paragraph: tbPara }] } };
  const phantom = { text: '￼', props: DEF(), textbox: { paras: [{ runs: [], props: {} }], widthTwips: 1, heightTwips: 1, blocks: [] } };
  const imgRun = {
    text: '￼', props: DEF(),
    image: {
      data: new Uint8Array([1, 2, 3]), ext: 'png', widthTwips: 10, heightTwips: 20,
      style: { geom: 'rect', fill: '4472C4|lumMod=75000', cust: [{ w: 1, h: 2, fill: true, stroke: false, cmds: [{ op: 'M', x: 0, y: 0 }, { op: 'Z' }] }], adj: { adj: 5000, 2: 1 } },
    },
  };
  const para2 = { runs: [tbRun, phantom, imgRun], props: {} };
  const headerPara = { runs: [{ text: 'H', props: DEF() }], props: {} };
  const headerBlocks = [{ type: 'paragraph', paragraph: headerPara }];
  const fnPara = { runs: [{ text: 'note', props: DEF() }], props: {} };
  const sectPr = { pageWidthTwips: 11906, pageHeightTwips: 16838, marginLeftTwips: 1701, marginRightTwips: 1701, marginTopTwips: 1417, marginBottomTwips: 1417 };
  return {
    blocks: [{ type: 'paragraph', paragraph: para1 }, table, { type: 'paragraph', paragraph: para2 }],
    sections: [{
      sectPr, sect: { type: 'nextPage' }, blockStart: 0, blockEnd: 2,
      headers: { default: [headerPara] }, footers: {},
      headerRefs: [{ type: 'default', rid: 'rId8', path: 'word/header1.xml' }], footerRefs: [],
      headerBlocks: { default: headerBlocks }, footerBlocks: {},
    }],
    footnotes: new Map([[1, [fnPara]]]),
    endnotes: new Map(),
    comments: [{ id: 0, author: undefined, text: 'c' }],
    styles: new Map([['Normal', { para: {}, run: { bold: false }, name: 'Normal', type: 'paragraph' }]]),
    bookmarks: new Map([['_Toc1', 0]]),
    bookmarkOffsets: new Map([['_Toc1', 3]]),
    bookmarkEnds: new Map([['_Toc1', { block: 0, offset: 5 }]]),
    formatting: {
      styles: { docDefaults: {}, styles: { Normal: { styleId: 'Normal', type: 'paragraph', name: 'Normal' }, 10: { styleId: '10', type: 'paragraph' } }, defaults: { paragraph: 'Normal' } },
      numbering: { abstractNums: {}, nums: {}, picBullets: {} },
      theme: { major: { scripts: {} }, minor: { scripts: { Jpan: 'X' }, latin: 'Aptos' }, colors: { dk1: '000000' } },
      settings: { defaultTabStop: 720, compatibilityMode: 15, compat: {}, evenAndOddHeaders: false, mirrorMargins: false, autoHyphenation: false, doNotHyphenateCaps: false, displayBackgroundShape: false, updateFields: false },
      fontTable: {},
    },
    footnoteBlocks: new Map([[1, [{ type: 'paragraph', paragraph: fnPara }]]]),
    endnoteBlocks: new Map(),
    noteSpecialIds: { footnote: [-1, 0], endnote: [] },
    coreProps: { created: '2024-01-01T00:00:00Z' },
  };
}

export const DOC_XML = '<w:document>é<w:body><w:p><w:r><w:t>Olá</w:t></w:r></w:p><w:tbl/></w:body></w:document>';

export function sampleSources() {
  const paraRaw = '<w:p><w:r><w:t>Olá</w:t></w:r></w:p>';
  return {
    next: 3,
    paras: new Map([[1, { raw: paraRaw, open: '<w:p>', at: DOC_XML.indexOf(paraRaw), segs: [{ raw: '<w:r><w:t>Olá</w:t></w:r>', sids: [2], open: true, link: 'https://x' }] }]]),
    runs: new Map([[2, { rPr: '<w:rPr/>' }]]),
    tables: new Map([[3, { raw: '<w:tbl/>', open: '<w:tbl>', at: DOC_XML.indexOf('<w:tbl/>') }]]),
    trs: new Map([[4, { open: '<w:tr>', pre: [{ raw: '<w:sdt>', open: 37 }], post: [{ raw: '</w:sdt>', close: 37 }] }]]),
    tcs: new Map([[5, { open: '<w:tc>', groups: [{ raw: '<w:p/>', blocks: [0] }] }]]),
    stories: new Map([['word/document.xml', { open: '<w:document>é<w:body>', close: '</w:body></w:document>', groups: [{ raw: paraRaw, blocks: [1], sdtKey: 's7', inner: [] }] }]]),
    txbxGroups: [],
    noteParts: new Map([['word/footnotes.xml', { open: '<w:footnotes>', close: '</w:footnotes>', items: [{ id: -1, raw: '<w:footnote/>', separator: true }], refsAtLoad: new Set([1]) }]]),
    commentIds: new Set([0]),
    commentsSig: '[{"id":0,"text":"c"}]',
  };
}

const run = (fn) => {
  const s = new Stream({ trace: !!traceDir });
  const o = new Stream({ trace: !!traceDir });
  const c = new Canon(s, { order: o, parts: [DOC_XML] });
  fn(c);
  return [s, o];
};

const [full, order] = run((c) => c.document(sampleDocument()));
const [src] = run((c) => c.sources(sampleSources()));
const lines = [
  '# canon self-test: the TS walker over js/canon-selftest.mjs sampleDocument()/sampleSources()',
  `full=${full.digest().hex}`,
  `order=${order.digest().hex}`,
  `sources=${src.digest().hex}`,
];
writeFileSync(join(here, '..', 'tests', 'canon', 'selftest.expected'), lines.join('\n') + '\n');
if (traceDir) {
  writeFileSync(join(traceDir, 'ts-full.trace'), full.trace());
  writeFileSync(join(traceDir, 'ts-order.trace'), order.trace());
  writeFileSync(join(traceDir, 'ts-sources.trace'), src.trace());
}
console.log(lines.join('\n'));
