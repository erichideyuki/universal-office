// The regex sites of frontier R5 (fulldoc.ts `readCoreProps`, blank.ts `LANG_TAG`, inspect.ts
// `runHasContent`) against V8, checked by tests/regex_r5.rs. A row is `[id, regex | function, op]`
// (see js/regex-diff.mjs). Not listed: `openOf`'s `/\s*\/>$/` (R6, r6.mjs); the relationship target
// `.replace(/^\.\//, '')` of `readFullDocument` and fonts.ts `.replace(/^\.?\//, '')` (replaced by the
// OPC rule of uo-opc D6: no matcher); fonts.ts `fontKey.replace(/[{}-]/g, '')` (uo-fonts `odttf`).
import { INPUTS as BASE } from './templates.mjs';

/** fulldoc.ts:777 — `new RegExp('<' + tag + '[^>]*>([^<]*)<').exec(xml)`, group 1 (the tags are constants). */
const core = (tag) => new RegExp('<' + tag + '[^>]*>([^<]*)<');

/** [site id, the TS regex (or function), operation]. */
export const SITES = [
  ['r1.core_created', core('dcterms:created'), 'group'],
  ['r1.core_modified', core('dcterms:modified'), 'group'],
  ['r1.core_printed', core('cp:lastPrinted'), 'group'],
  // blank.ts:27 `isLanguageTag` (the `!!value &&` part is the caller's)
  ['r2.lang_tag', /^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$/, 'test'],
  // inspect.ts:45 `runHasContent`: the last line, a text that is not only object marks and white space
  ['r3.run_has_text', (s) => s.replace(/￼/g, '').trim().length > 0, 'fn'],
];

const lang = [
  'pt', 'pt-BR', 'zh-Hans-CN', 'es-419', 'abc', 'abcd', 'a', 'p1', 'pt-', '-pt', 'pt--BR', 'pt-B', 'pt-123456789', 'pt-12345678', 'pt_BR', 'EN-us', 'en-US-x-lab12345', 'en-US-lab12345', 'pt-BR\n', 'pt-BR ', ' pt-BR',
  'é', 'pt-é1', 'ptx-BR', 'pt-BR-', 'pt-br-1234-5678', 'pt-ab-cd-ef-gh-ij-kl', 'PT-BR', '日本', 'pt-𝒳𝒳', 'a-b', 'ab-c', 'ab-cd', 'abc-de', 'abcde',
];
const xml = (inner) => `<cp:coreProperties>${inner}</cp:coreProperties>`;
const core_inputs = [
  xml('<dcterms:created xsi:type="dcterms:W3CDTF">2020-01-02T03:04:05Z</dcterms:created>'),
  xml('<dcterms:created>x</dcterms:created><dcterms:created>y</dcterms:created>'),
  xml('<dcterms:modified> m </dcterms:modified>'),
  xml('<cp:lastPrinted>2019</cp:lastPrinted>'),
  xml('<dcterms:created/>'),
  xml('<dcterms:created/>\n  <dcterms:modified>m</dcterms:modified>'),
  xml('<dcterms:created></dcterms:created>'),
  xml('<dcterms:created2>no</dcterms:created2><dcterms:createdX a="b">yes</dcterms:createdX>'),
  xml('<dcterms:created>never closed'),
  xml('<dcterms:created a="1>2">q</dcterms:created>'),
  xml('<dcterms:created><b>inner</b>tail</dcterms:created>'),
  xml('<dcterms:created a=">">z</dcterms:created>'),
  '<dcterms:created', '<dcterms:created>', '<dcterms:created>x', '<dcterms:created>x<', '<dcterms:created>é😀<', '<dcterms:created>\n<', '<<dcterms:created>x<',
  '<dcterms:created><dcterms:created>x<', '<x><dcterms:modified>a</dcterms:modified>', '<cp:lastPrinted/>q<', '<cp:lastPrinted a="b"/>text<x/>', 'dcterms:created>x<', '<DCTERMS:created>x<',
];
const text = ['￼', '￼￼', ' ', '\t', '﻿', ' ', '\u0085', '​', ' ', '￼ ￼', '￼a', 'a￼', 'a', '  a  ', '　￼', '�', '᠎', ' ', ' ', ' ', '￼\u0000'];

export const INPUTS = [...new Set([...BASE, ...lang, ...core_inputs, ...text])];
