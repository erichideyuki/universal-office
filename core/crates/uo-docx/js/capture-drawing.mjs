// Capture of frontier R3 (uo-docx D1a: drawing.ts, vml-shape.ts, page-background.ts and
// watermark.ts read functions) from the frozen TS engine, facet `drawing` of the brief:
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-drawing.mjs
//     [--only <substring>] [--trace <key>]
//
// Inputs: every git-tracked .docx/.docm/.dotx of the checkout (the 2,742 oracle documents of
// docx-layout and the other fixtures), and a seeded generated corpus of attribute quirks (written
// to tests/parity/drawing.quirks.txt, one document fragment per line, so that the Rust replay
// needs no generator). Per XML part, every target element in document order is one record
// `<docx path>#<part>#<ordinal>` (quirk cases: `quirk/<n>#<ordinal>`), with the facets:
//
//   anchor     wp:anchor        -> readWpAnchor
//   sppr       wps:wsp          -> readSpPr(wps:spPr, wps:style)   (pic:pic: readSpPr(pic:spPr))
//   cust       a:custGeom       -> readCustGeom(cg, 1905000, 952500)
//   vmlstyle0  v:shape|rect|roundrect|oval|line -> parseVmlStyle(@style)
//   vmlstyle   same             -> vmlShapeStyle(el, 'rect')
//   vmlanchor  same             -> vmlAnchor(parseVmlStyle(@style), findDeep(el, 'w10:wrap'))
//   vmltb      same             -> vmlTextboxToDrawing(el, 1 + spid, lang by ordinal) | '!throw'
//   single     w:pict           -> the index of vmlSingleTextbox(pict) among its elements, or -1
//   wm         w:pict           -> { is: isWatermarkXml(xml), info: readWatermarkXml(xml, vmlColor) }
//   bg         w:background     -> readPageBackground(el, rid => stub image)
//
// A facet is the SHA-256 (first 16 bytes, hex) of the canonical walk (js/canon.mjs) of the
// function's result; a function that throws is the string '!throw'. Output:
// tests/parity/drawing.capture (format: core/crates/uo-testkit/src/capture.rs).
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Rng, Stream, fffd, importFrom, tsRoot } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = tsRoot();
const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : null;
const traceKey = argv.includes('--trace') ? argv[argv.indexOf('--trace') + 1] : null;

const core = 'write/packages/docx-core/dist/';
const { DocxPackage } = await importFrom(root, `${core}package.js`);
const { parseXml, childElements, firstChild } = await importFrom(root, `${core}xml.js`);
const D = await importFrom(root, `${core}drawing.js`);
const { readPageBackground } = await importFrom(root, `${core}page-background.js`);
const { isWatermarkXml, readWatermarkXml } = await importFrom(root, `${core}watermark.js`);
const { vmlTextboxToDrawing, vmlSingleTextbox } = await importFrom(root, `${core}vml-shape.js`);
const tsCommit = execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();

const VML_SHAPES = new Set(['v:shape', 'v:rect', 'v:roundrect', 'v:oval', 'v:line']);
const LANGS = ['pt', 'en', 'es'];

const digest = (fn, trace) => {
  const s = new Stream({ trace: !!trace });
  try { fn(new Canon(s)); } catch {
    const t = new Stream({ trace: !!trace });
    new Canon(t).str('!throw');
    return [t.digest().short, t.trace()];
  }
  return [s.digest().short, s.trace()];
};

/** The target elements of a parsed part, in document order, with their facets. */
function targetsOf(rootEl, text) {
  const out = [];
  const stack = [rootEl];
  while (stack.length) {
    const el = stack.pop();
    const n = el.name;
    const facets = [];
    if (n === 'wp:anchor') facets.push(['anchor', (c) => c.value(D.readWpAnchor(el), 'x')]);
    else if (n === 'wps:wsp') facets.push(['sppr', (c) => c.value(D.readSpPr(firstChild(el, 'wps:spPr'), firstChild(el, 'wps:style')), 'x')]);
    else if (n === 'pic:pic') facets.push(['sppr', (c) => c.value(D.readSpPr(firstChild(el, 'pic:spPr'), undefined), 'x')]);
    else if (n === 'a:custGeom') facets.push(['cust', (c) => c.value(D.readCustGeom(el, 1905000, 952500), 'x')]);
    else if (VML_SHAPES.has(n)) {
      facets.push(['vmlstyle0', (c) => c.value(D.parseVmlStyle(el.attrs['style']), 'x')]);
      facets.push(['vmlstyle', (c) => c.value(D.vmlShapeStyle(el, 'rect'), 'x')]);
      facets.push(['vmlanchor', (c) => c.value(D.vmlAnchor(D.parseVmlStyle(el.attrs['style']), D.findDeepDrawing(el, 'w10:wrap')), 'x')]);
      const k = out.length;
      facets.push(['vmltb', (c) => c.value(vmlTextboxToDrawing(el, 1 + (parseInt((el.attrs['o:spid'] ?? '').replace(/\D/g, ''), 10) || 0), LANGS[k % 3]), 'x')]);
    } else if (n === 'w:pict') {
      facets.push(['single', (c) => { const s = vmlSingleTextbox(el); c.num(s ? childElements(el).indexOf(s) : -1); }]);
      const xml = text.slice(el.s, el.e);
      facets.push(['wm', (c) => c.value({ is: isWatermarkXml(xml), info: readWatermarkXml(xml, D.vmlColor) ?? null }, 'x')]);
    } else if (n === 'w:background') {
      facets.push(['bg', (c) => c.value(readPageBackground(el, (rid) => (rid.length % 2 === 0 ? { data: new TextEncoder().encode(`img:${rid}`), ext: 'png' } : undefined)), 'x')]);
    }
    if (facets.length) out.push(facets);
    const kids = childElements(el);
    for (let i = kids.length - 1; i >= 0; i--) stack.push(kids[i]);
  }
  return out;
}

const cap = new CaptureWriter(['uo-docx R3 drawing facet: readWpAnchor, readSpPr, readCustGeom, VML styles and anchors, vmlTextboxToDrawing, page background, watermark read']);
cap.meta('ts-commit', tsCommit);
let records = 0;
let docs = 0;

function record(keyBase, parts) {
  // parts: [partName, text]
  let ordinal = 0;
  for (const [name, text] of parts) {
    let rootEl;
    try { rootEl = parseXml(text, { positions: true }); } catch { continue; }
    for (const facets of targetsOf(rootEl, text)) {
      const key = `${keyBase}#${name}#${ordinal++}`;
      if (only && !key.includes(only)) continue;
      for (const [facet, fn] of facets) {
        const [d, trace] = digest(fn, traceKey === key);
        cap.set(key, facet, d);
        if (traceKey === key) console.log(`--- ${key} ${facet}\n${trace}`);
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
    if (!/^word\/[^/]*\.xml$/.test(name) && !name.endsWith('.vml')) continue;
    const text = pkg.getPartText(name);
    if (text !== undefined) parts.push([name, text]);
  }
  docs++;
  record(rel, parts);
}

// ── the generated attribute-quirk corpus ──
const rng = new Rng(0x3d7a11e5);
const pick = (a) => a[rng.below(a.length)];
const chance = (n) => rng.below(n) === 0;
const attr = (k, v) => (v === undefined ? '' : ` ${k}="${v}"`);
const opt = (v, miss = 6) => (chance(miss) ? undefined : v);
const INTS = ['0', '1', '-1', '635', '12700', '914400', '3175', '1e3', ' 12 ', '12pt', '0x10', '-0', 'NaN', '', '99999999999999999999', '2147483648', '-2147483649', '.5', '1.9', 'abc', '60000', '5400000', '100000', '21600'];
const BOOLS = ['1', '0', 'true', 'false', 'on', '', 'TRUE', 'x'];
const COLORS = ['4472c4', 'FF0000', '00ff00', '12345', 'zz', '', '0A0B0C', 'ffffff', 'abcdefg', '@x', 'é'];
const SCHEME = ['accent1', 'accent6', 'tx1', 'tx2', 'bg1', 'bg2', 'dk1', 'lt2', 'phClr', 'text1', 'nope', ''];
const MODS = ['alpha', 'lumMod', 'lumOff', 'tint', 'shade', 'satMod', 'gray', 'comp', 'bogus'];
const MODVALS = ['75000', '25000', '50000', '-20000', '0', '100000', '1e3', 'abc', '', ' 5000'];
const PRSTS = ['rect', 'roundRect', 'ellipse', 'line', '', 'triangle', 'x y', '__proto__', 'constructor'];
const NAMES = ['adj', 'adj1', 'adj2', '1', '2', '__proto__', 'constructor', '', 'val', 'w'];
const FMLAS = ['val 5000', 'val -12', 'val  7', 'val\t3', 'VAL 1', 'val 1x', 'val-3', 'val', '', 'val  -', 'val 12345678901234567890'];

function colorEl() {
  const kind = pick(['srgbClr', 'schemeClr', 'sysClr', 'prstClr', 'scrgbClr']);
  let a = '';
  if (kind === 'srgbClr') a = attr('val', opt(pick(COLORS)));
  else if (kind === 'schemeClr') a = attr('val', opt(pick(SCHEME)));
  else if (kind === 'sysClr') a = attr('val', opt(pick(['window', 'windowText', 'btnFace']))) + attr('lastClr', chance(2) ? pick(COLORS) : undefined);
  else if (kind === 'prstClr') a = attr('val', opt(pick(['black', 'white', 'red', 'gray', 'grey', ''])));
  else a = ' r="50000" g="0" b="0"';
  let mods = '';
  for (let k = rng.below(4); k > 0; k--) mods += `<a:${pick(MODS)}${attr('val', chance(5) ? undefined : pick(MODVALS))}/>`;
  return mods ? `<a:${kind}${a}>${mods}</a:${kind}>` : `<a:${kind}${a}/>`;
}

function gdLst(tag) {
  let s = '';
  for (let k = rng.below(5); k > 0; k--) {
    const op = pick(['*/', '+-', '+/', '?:', 'abs', 'at2', 'cat2', 'sat2', 'cos', 'sin', 'tan', 'max', 'min', 'mod', 'pin', 'sqrt', 'val', 'bogus', '']);
    const args = Array.from({ length: rng.below(5) }, () => pick(['w', 'h', 'ss', 'ls', 'wd2', 'hd4', 'cd4', '3cd4', 'adj', 'g1', '5400000', '0', '-3', '1e3', 'x', '7', 'vc', 'hc'])).join(pick([' ', '  ', '\t']));
    s += `<a:gd${attr('name', opt(pick(['g1', 'g2', 'adj', 'x', '__proto__', '']), 8))}${attr('fmla', opt(`${op} ${args}`, 10))}/>`;
  }
  return s ? `<a:${tag}>${s}</a:${tag}>` : '';
}

function custGeom() {
  const pt = () => `<a:pt x="${pick(['0', 'w', 'h', 'g1', '100', '-5', 'x', '1e3', ''])}" y="${pick(['0', 'w', 'h', 'g2', '200', 'hd2', 'ss'])}"/>`;
  let paths = '';
  for (let p = rng.below(3); p > 0; p--) {
    let cmds = '';
    for (let k = rng.below(6); k > 0; k--) {
      cmds += pick([
        () => `<a:moveTo>${pt()}</a:moveTo>`, () => `<a:lnTo>${pt()}</a:lnTo>`, () => '<a:moveTo/>',
        () => `<a:cubicBezTo>${pt()}${pt()}${pt()}</a:cubicBezTo>`, () => `<a:quadBezTo>${pt()}${pt()}</a:quadBezTo>`,
        () => `<a:arcTo${attr('wR', pick(['w', '100', 'abc']))}${attr('hR', pick(['h', '50']))}${attr('stAng', pick(['0', '5400000', 'cd4']))}${attr('swAng', pick(['10800000', 'cd2', '-5400000']))}/>`,
        () => '<a:close/>',
      ])();
    }
    paths += `<a:path${attr('w', opt(pick(INTS)))}${attr('h', opt(pick(INTS)))}${attr('fill', opt(pick(['none', 'norm', '', 'lighten'])))}${attr('stroke', opt(pick(['0', 'false', '1', 'true', ''])))}>${cmds}</a:path>`;
  }
  return `<a:custGeom>${gdLst('avLst')}${gdLst('gdLst')}<a:pathLst>${paths}</a:pathLst></a:custGeom>`;
}

function fillEl() {
  return pick([
    () => '<a:noFill/>',
    () => `<a:solidFill>${colorEl()}</a:solidFill>`,
    () => {
      let gs = '';
      for (let k = 2 + rng.below(3); k > 0; k--) gs += `<a:gs${attr('pos', opt(pick(['0', '50000', '100000', '25000', 'x', '-5', '1e3', '']), 10))}>${chance(14) ? '' : colorEl()}</a:gs>`;
      const lin = chance(3) ? '' : `<a:lin${attr('ang', opt(pick(INTS)))}${attr('scaled', opt(pick(BOOLS)))}/>`;
      const path = chance(3) ? `<a:path${attr('path', opt(pick(['circle', 'rect', 'shape'])))}>${chance(2) ? `<a:fillToRect${attr('l', opt(pick(INTS)))}${attr('t', opt(pick(INTS)))}/>` : ''}</a:path>` : '';
      return `<a:gradFill${attr('rotWithShape', opt(pick(BOOLS)))}><a:gsLst>${gs}</a:gsLst>${lin}${path}</a:gradFill>`;
    },
    () => `<a:pattFill${attr('prst', opt(pick(['pct5', 'dkHorz', ''])))}>${chance(3) ? `<a:fgClr>${colorEl()}</a:fgClr>` : ''}${chance(3) ? `<a:bgClr>${colorEl()}</a:bgClr>` : ''}</a:pattFill>`,
    () => '<a:blipFill><a:blip r:embed="rId1"/></a:blipFill>',
    () => '<a:grpFill/>',
  ])();
}

function lnEl() {
  const fill = pick([() => '<a:noFill/>', () => `<a:solidFill>${colorEl()}</a:solidFill>`, () => '', () => `<a:solidFill>${colorEl()}</a:solidFill>`])();
  return `<a:ln${attr('w', opt(pick(INTS)))}>${fill}${chance(2) ? `<a:prstDash${attr('val', opt(pick(['solid', 'dash', 'sysDot', '', 'x'])))}/>` : ''}</a:ln>`;
}

function effectLst() {
  let s = '';
  if (chance(2)) s += `<a:glow${attr('rad', opt(pick(INTS)))}>${chance(5) ? '' : colorEl()}</a:glow>`;
  if (chance(3)) s += `<a:softEdge${attr('rad', opt(pick(INTS)))}/>`;
  if (chance(3)) s += `<a:innerShdw${attr('blurRad', opt(pick(INTS)))}${attr('dist', opt(pick(INTS)))}${attr('dir', opt(pick(INTS)))}>${colorEl()}</a:innerShdw>`;
  if (chance(3)) s += `<a:reflection${attr('blurRad', opt(pick(INTS)))}${attr('stA', opt(pick(INTS)))}${attr('stPos', opt(pick(INTS)))}${attr('endA', opt(pick(INTS)))}${attr('endPos', opt(pick(INTS)))}${attr('dist', opt(pick(INTS)))}${attr('dir', opt(pick(INTS)))}${attr('sy', opt(pick(INTS)))}${attr('algn', opt(pick(['b', 'tl', ''])))}${attr('rotWithShape', opt(pick(BOOLS)))}/>`;
  if (chance(2)) s += `<a:outerShdw${attr('blurRad', opt(pick(INTS)))}${attr('dist', opt(pick(INTS)))}${attr('dir', opt(pick(INTS)))}${attr('sx', opt(pick(INTS), 3))}${attr('sy', opt(pick(INTS), 3))}${attr('kx', opt(pick(INTS), 3))}${attr('ky', opt(pick(INTS), 3))}${attr('algn', opt(pick(['b', 'tl', ''])))}${attr('rotWithShape', opt(pick(BOOLS)))}>${chance(5) ? '' : colorEl()}</a:outerShdw>`;
  return `<a:effectLst>${s}</a:effectLst>`;
}

function spPrFragment() {
  const geom = chance(3) ? custGeom() : `<a:prstGeom${attr('prst', opt(pick(PRSTS), 3))}><a:avLst>${Array.from({ length: rng.below(4) }, () => `<a:gd${attr('name', opt(pick(NAMES), 8))}${attr('fmla', opt(pick(FMLAS), 8))}/>`).join('')}</a:avLst></a:prstGeom>`;
  const xfrm = chance(5) ? '' : `<a:xfrm${attr('rot', opt(pick(INTS)))}${attr('flipH', opt(pick(BOOLS)))}${attr('flipV', opt(pick(BOOLS)))}><a:off x="0" y="0"/>${chance(5) ? '' : `<a:ext${attr('cx', opt(pick(INTS)))}${attr('cy', opt(pick(INTS)))}/>`}</a:xfrm>`;
  const style = chance(2) ? `<wps:style><a:lnRef${attr('idx', opt(pick(INTS)))}>${colorEl()}</a:lnRef><a:fillRef${attr('idx', opt(pick(['0', '1', '2', '3', '1001', '1002', 'x', ''])))}>${colorEl()}</a:fillRef><a:effectRef${attr('idx', opt(pick(INTS)))}>${colorEl()}</a:effectRef></wps:style>` : '';
  const body = `${xfrm}${geom}${chance(6) ? '' : fillEl()}${chance(3) ? lnEl() : ''}${chance(3) ? effectLst() : ''}`;
  return `<wps:wsp><wps:spPr>${body}</wps:spPr>${style}</wps:wsp>`;
}

function gradSp() {
  let gs = '';
  const n = 2 + rng.below(4);
  for (let k = 0; k < n; k++) {
    const kind = pick(['srgbClr', 'schemeClr']);
    gs += `<a:gs pos="${pick(['0', '100000', '50000', '25000', '75000', '20000', '-5', '1e3', 'x', '100000', '0'])}"><a:${kind} val="${kind === 'srgbClr' ? pick(['4472c4', 'FF0000', '00ff00', 'zz']) : pick(['accent1', 'tx1', 'bg2'])}">${chance(3) ? `<a:alpha val="${pick(MODVALS)}"/>` : ''}${chance(4) ? `<a:lumMod val="${pick(MODVALS)}"/>` : ''}</a:${kind}></a:gs>`;
  }
  const lin = chance(4) ? '' : `<a:lin ang="${pick(INTS)}"${attr('scaled', opt(pick(BOOLS)))}/>`;
  const path = chance(4) ? `<a:path${attr('path', opt(pick(['circle', 'rect', 'shape'])))}>${chance(2) ? `<a:fillToRect${attr('l', opt(pick(INTS)))}${attr('t', opt(pick(INTS)))}${attr('r', opt(pick(INTS)))}${attr('b', opt(pick(INTS)))}/>` : ''}</a:path>` : '';
  return `<wps:wsp><wps:spPr><a:prstGeom prst="rect"/><a:gradFill${attr('rotWithShape', opt(pick(BOOLS)))}><a:gsLst>${gs}</a:gsLst>${lin}${path}</a:gradFill>${chance(2) ? lnEl() : ''}</wps:spPr></wps:wsp>`;
}

function shadowSp() {
  const shd = `<a:outerShdw${attr('blurRad', opt(pick(INTS), 3))}${attr('dist', opt(pick(INTS), 3))}${attr('dir', opt(pick(INTS), 3))}${attr('sx', opt(pick(['100000', '100000', '90000']), 2))}${attr('rotWithShape', opt(pick(BOOLS)))}>${chance(8) ? '' : colorEl()}</a:outerShdw>`;
  const eff = chance(4) ? `<a:effectLst><a:glow rad="12700">${colorEl()}</a:glow>${shd}</a:effectLst>` : `<a:effectLst>${shd}</a:effectLst>`;
  return `<wps:wsp><wps:spPr><a:prstGeom prst="rect"/>${chance(2) ? fillEl() : ''}${chance(2) ? lnEl() : ''}${eff}</wps:spPr></wps:wsp>`;
}

function anchorFragment() {
  const pos = (tag) => {
    const inner = pick([
      () => `<wp:align>${pick(['left', 'center', ' right ', '', 'x'])}</wp:align>`,
      () => `<wp:posOffset>${pick(INTS)}</wp:posOffset>`,
      () => `<wp14:pctPosHOffset>${pick(INTS)}</wp14:pctPosHOffset>`,
      () => '',
      () => `<wp:align>left</wp:align><wp:posOffset>12</wp:posOffset>`,
    ])();
    const el = `<${tag}${attr('relativeFrom', opt(pick(['page', 'margin', 'column', 'paragraph', 'x', ''])))}>${inner}</${tag}>`;
    return chance(4) ? `<mc:AlternateContent><mc:Choice Requires="wp14">${chance(2) ? el : ''}</mc:Choice><mc:Fallback>${chance(2) ? el : ''}</mc:Fallback></mc:AlternateContent>` : chance(8) ? '' : el;
  };
  const poly = () => {
    const rect = '<wp:start x="0" y="0"/><wp:lineTo x="0" y="21600"/><wp:lineTo x="21600" y="21600"/><wp:lineTo x="21600" y="0"/><wp:lineTo x="0" y="0"/>';
    if (chance(3)) return `<wp:wrapPolygon edited="0">${rect}</wp:wrapPolygon>`;
    let p = '<wp:start x="0" y="0"/>';
    for (let k = rng.below(7); k > 0; k--) p += `<wp:lineTo${attr('x', opt(pick(INTS)))}${attr('y', opt(pick(INTS)))}/>`;
    return chance(8) ? '' : `<wp:wrapPolygon>${p}</wp:wrapPolygon>`;
  };
  const wraps = Array.from({ length: 1 + rng.below(2) }, () => pick([
    () => '<wp:wrapNone/>', () => `<wp:wrapSquare${attr('wrapText', opt(pick(['bothSides', 'left', 'largest', ''])))}/>`,
    () => `<wp:wrapTight${attr('wrapText', opt(pick(['bothSides', 'right'])))}>${poly()}</wp:wrapTight>`,
    () => `<wp:wrapThrough${attr('wrapText', opt(pick(['bothSides', 'right'])))}>${poly()}</wp:wrapThrough>`, () => '<wp:wrapTopAndBottom/>',
  ])()).join('');
  return `<wp:anchor${attr('behindDoc', opt(pick(BOOLS)))}${attr('distT', opt(pick(INTS)))}${attr('distB', opt(pick(INTS)))}${attr('distL', opt(pick(INTS)))}${attr('distR', opt(pick(INTS)))}${attr('simplePos', opt(pick(BOOLS)))}${attr('relativeHeight', opt(pick(INTS)))}${attr('layoutInCell', opt(pick(BOOLS)))}${attr('allowOverlap', opt(pick(BOOLS)))}>`
    + `${chance(3) ? `<wp:simplePos${attr('x', opt(pick(INTS)))}${attr('y', opt(pick(INTS)))}/>` : ''}${pos('wp:positionH')}${pos('wp:positionV')}${wraps}`
    + `${chance(3) ? `<wp:effectExtent${attr('l', opt(pick(INTS)))}${attr('t', opt(pick(INTS)))}${attr('r', opt(pick(INTS)))}${attr('b', opt(pick(INTS)))}/>` : ''}</wp:anchor>`;
}

const LENS = ['12pt', '1in', '5cm', '10mm', '3pc', '2em', '16px', '16', '-.5in', '1.2.3pt', '.', '-', '', '5 pt', '5 pt', ' 7in ', '1e3pt', 'abc', '100%', '0', '-0pt', '99999999999999999999pt', '1.5', '12PT', ' 12pt', '12pt ', '﻿5in', '5pt', '5　cm', '5 pt'];
const VCOLORS = ['#ff0000', '#f00', '#FFF', 'red', 'RED', 'window', '#12', '#1234567', '#xyz', '#abc def', 'fill darken(128)', '', ' ', 'silver', 'grey', 'nope'];
function vmlStyle() {
  const decls = [];
  const put = (k, v) => { if (!chance(4)) decls.push(`${k}:${v}`); };
  put('position', pick(['absolute', 'absolute', 'relative', 'Absolute', '']));
  put('margin-left', pick(LENS));
  put('margin-top', pick(LENS));
  put('left', pick(LENS));
  put('top', pick(LENS));
  put('width', pick(LENS));
  put('height', pick(LENS));
  put('z-index', pick(INTS));
  put('mso-position-horizontal', pick(['center', 'absolute', 'left', 'constructor', '']));
  put('mso-position-vertical', pick(['center', 'absolute', 'bottom', '']));
  put('mso-position-horizontal-relative', pick(['page', 'margin', 'text', 'char', 'left-margin-area', 'constructor', '__proto__', 'toString', 'nope', '']));
  put('mso-position-vertical-relative', pick(['page', 'margin', 'text', 'line', 'top-margin-area', 'inner-margin-area', 'hasOwnProperty', 'nope', '']));
  put('mso-wrap-distance-left', pick(LENS));
  put('mso-wrap-distance-top', pick(LENS));
  put('mso-wrap-distance-right', pick(LENS));
  put('mso-wrap-distance-bottom', pick(LENS));
  put('rotation', pick(INTS));
  put('flip', pick(['x', 'y', 'xy', '', 'z']));
  put('visibility', pick(['hidden', 'visible']));
  put('v-text-anchor', pick(['middle', 'bottom', 'top', 'bottom-baseline', '']));
  put('mso-wrap-style', pick(['none', 'square']));
  put('__proto__', 'x');
  put('MARGIN-LEFT', pick(LENS));
  rng.below(2) && decls.reverse();
  return decls.join(pick([';', ' ; ', ';\n']));
}

function vmlPict() {
  const tag = pick(['v:shape', 'v:rect', 'v:roundrect', 'v:oval', 'v:line']);
  const attrs = attr('id', opt(pick(['_x0000_s1026', 'Caixa 1', 'PowerPlusWaterMarkObject1', '', 'a&amp;b'])))
    + attr('o:spid', opt(pick(['_x0000_s1026', 'x', '', '_x0000_s99999999999999999999'])))
    + attr('style', opt(vmlStyle(), 12))
    + attr('filled', opt(pick(['f', 'false', 't', '', 'true']), 4)) + attr('stroked', opt(pick(['f', 'false', 't', '']), 4))
    + attr('fillcolor', opt(pick(VCOLORS), 3)) + attr('strokecolor', opt(pick(VCOLORS), 3)) + attr('strokeweight', opt(pick(['1pt', '2', '.5pt', '3t', '', '1e3', '2in', 'x']), 3))
    + attr('arcsize', opt(pick(['.25', '5461f', 'x', '', '0']), 3)) + attr('alt', opt(pick(['texto', 'a&quot;b&lt;c', '', 'x & y']), 3)) + attr('o:title', opt(pick(['t', '', 'a&amp;b']), 3));
  const kids = (chance(2) ? `<v:fill${attr('on', opt(pick(['f', 't', 'false']), 3))}${attr('color', opt(pick(VCOLORS), 3))}${attr('color2', opt(pick(VCOLORS), 3))}${attr('opacity', opt(pick(['.5', '32768f', 'x', '', '1', '-.5', 'f']), 3))}${attr('type', opt(pick(['gradient', 'gradientRadial', 'solid', 'frame'])))}${attr('angle', opt(pick(INTS)))}/>` : '')
    + (chance(2) ? `<v:stroke${attr('on', opt(pick(['f', 't']), 3))}${attr('color', opt(pick(VCOLORS), 3))}${attr('weight', opt(pick(['1pt', '2']), 3))}${attr('dashstyle', opt(pick(['dash', 'solid', 'a&quot;b', 'x&lt;y', 'a&amp;b', ''])))}${attr('linestyle', opt(pick(['single', 'thinThin', 'thickThin', 'constructor', '__proto__', 'nope', ''])))}/>` : '')
    + (chance(5) ? '' : `<v:textbox${attr('style', opt(pick(['layout-flow:vertical;mso-layout-flow-alt:bottom-to-top', 'layout-flow:vertical', 'layout-flow:vertical-ideographic', 'mso-fit-shape-to-text:t', 'layout-flow:x;mso-layout-flow-alt:y', ''])))}${attr('inset', opt(pick(['0,0,0,0', '7.2pt,3.6pt', ',,,', ' 1pt , 2pt ', 'x,y', '1in,,2cm,1e3'])))}><w:txbxContent/></v:textbox>`)
    + (chance(2) ? `<w10:wrap${attr('type', opt(pick(['square', 'tight', 'through', 'topAndBottom', 'none', ''])))}/>` : '');
  const shape = `<${tag}${attrs}>${kids}</${tag}>`;
  const extra = chance(5) ? `<v:shape${attr('id', 'x')}><v:textbox/></v:shape>` : chance(6) ? '<v:shapetype id="_x0000_t202"/>' : '';
  return `<w:pict>${extra}${shape}</w:pict>`;
}

function backgroundFragment() {
  const type = pick(['gradient', 'gradientRadial', 'pattern', 'frame', 'tile', 'solid', '']);
  const fill = `<v:fill${attr('type', opt(type, 8))}${attr('color2', opt(pick(VCOLORS), 3))}${attr('angle', opt(pick(INTS)))}${attr('focus', opt(pick(['100%', '-50%', '50', '49152f', '.5', 'x', '%', '', '1e3', ' 25% ', 'f'])))}`
    + `${attr('focusposition', opt(pick(['.5,.5', '1', ',', ' 1 , .25 ', '1,2,3', 'x,y', '', '0.5f,1f', ',.5'])))}${attr('colors', opt(pick(['0 #000082;19661f #66008f;1 #fff', '0 red', ' 0 red ; 1 blue ', 'x y;1', ';;', '1\t#f00', 'a'])))}`
    + `${attr('method', opt(pick(['linear sigma', 'none', ''])))}${attr('o:title', opt(pick(['', ' 5%', 'foto.png'])))}${attr('r:id', opt(pick(['rId1', 'rId22', '', 'r'])))}>${chance(3) ? `<o:fill${attr('type', pick(['gradientCenter', 'x']))}/>` : ''}</v:fill>`;
  const vbg = chance(8) ? '' : `<v:background${attr('fillcolor', opt(pick(VCOLORS), 3))}${attr('o:targetscreensize', opt(pick(['1024,768', '']), 3))}>${chance(8) ? '' : fill}</v:background>`;
  return `<w:background${attr('w:color', opt(pick(COLORS)))}${attr('w:themeColor', opt(pick(SCHEME), 3))}${attr('w:themeTint', opt(pick(['BF', ''])))}${attr('w:themeShade', opt(pick(['99', ''])))}>${vbg}</w:background>`;
}

function watermarkPict() {
  const kind = pick(['PowerPlusWaterMarkObject', 'WordPictureWatermark', 'PowerPlusWaterMarkObject', 'Other']);
  const id = attr('id', `${kind}${pick(['', '1', '12345', 'x'])}`);
  const style = `position:absolute;margin-left:0;margin-top:0;width:${pick(['468pt', '10in', '5cm', '30mm', '1e3pt', 'x', '', ' 20pt ', '.5pt'])};height:${pick(['117pt', '1in', '2.5cm', '', '10'])};${chance(2) ? `rotation:${pick(['315', '0', '-45', '180', '360', 'x', '90.4', '89.6', '1e3', ' 45 '])};` : ''}z-index:-251657216`;
  const shape = kind === 'WordPictureWatermark'
    ? `<v:shape${id}${attr('style', style)}><v:imagedata${attr('r:id', opt(pick(['rId1', ''])))}${attr('o:title', opt(pick(['x.png', 'a&amp;b&quot;', ''])))}${chance(2) ? ' gain="19661f"' : ''}${chance(4) ? ' blacklevel="22938f"' : ''}${chance(6) ? ' xgain="1"' : ''}/></v:shape>`
    : `<v:shape${id}${attr('style', style)}${attr('fillcolor', opt(pick(['silver', '#c0c0c0', '#f00', 'nope', ''])))}>${chance(2) ? `<v:fill${attr('opacity', pick(['.5', '1', '32768f', 'x', '']))}/>` : ''}`
      + `<v:textpath${attr('style', pick(['font-family:&quot;Calibri&quot;;font-size:1pt', 'font-family:&quot;Arial&quot;;font-size:40pt', "font-family:'Times';font-size:12.5pt", 'font-size:2pt', 'font-family:;font-size: ', 'font-family: ;', ';font-size:0.5pt;font-family:Cambria;', 'font-family:&quot;']))}${attr('string', opt(pick(['RASCUNHO', 'A &amp; B', 'x&lt;y&gt;', '', 'é😀'])))}/></v:shape>`;
  const wrap = chance(3) ? `${shape}<v:shape id="PowerPlusWaterMarkObject9" style="width:1pt"></v:shape>` : shape;
  return `<w:pict>${chance(3) ? '<v:shapetype id="_x0000_t136"/>' : ''}${wrap}</w:pict>`;
}

const GOODLEN = ['100pt', '2in', '5cm', '10mm', '1.5pc', '200px', '300', '7em', '12.5pt', ' 40pt ', '1e2'];
const INJECT = ['&lt;/wp:align&gt;&lt;x/&gt;', 'a&amp;b', 'c&lt;d', '&lt;/wp:positionH&gt;&lt;wp:positionH relativeFrom=&quot;page&quot;&gt;', 'left', 'center', '&quot;&gt;&lt;a:x/&gt;&lt;a:y a=&quot;'];
function vmlTextboxPict() {
  const decls = ['position:absolute', `width:${pick(GOODLEN)}`, `height:${pick(GOODLEN)}`];
  const put = (k, vals, miss = 3) => { if (!chance(miss)) decls.push(`${k}:${pick(vals)}`); };
  put('margin-left', LENS); put('margin-top', LENS); put('left', LENS); put('top', LENS);
  put('z-index', INTS);
  put('mso-position-horizontal', chance(4) ? INJECT : ['center', 'absolute', 'left', 'right', 'inside', ''], 2);
  put('mso-position-vertical', chance(4) ? INJECT : ['center', 'absolute', 'top', 'bottom'], 2);
  put('mso-position-horizontal-relative', ['page', 'margin', 'text', 'char', 'left-margin-area', 'right-margin-area', 'inner-margin-area', 'constructor', '__proto__', 'toString', 'valueOf', 'hasOwnProperty', 'nope', ''], 2);
  put('mso-position-vertical-relative', ['page', 'margin', 'text', 'line', 'top-margin-area', 'bottom-margin-area', 'outer-margin-area', 'isPrototypeOf', 'toLocaleString', 'nope'], 2);
  put('mso-wrap-distance-left', LENS); put('mso-wrap-distance-top', LENS); put('mso-wrap-distance-right', LENS); put('mso-wrap-distance-bottom', LENS);
  put('rotation', ['45', '-30', '0', '360', '725', 'x', '1e2', '0.4', '-0.4'], 2);
  put('flip', ['x', 'y', 'xy', ''], 2);
  put('visibility', ['hidden', 'visible'], 2);
  put('v-text-anchor', ['middle', 'bottom', 'top', 'bottom-baseline', 'middle-center'], 2);
  put('mso-wrap-style', ['none', 'square']);
  if (chance(2)) decls.reverse();
  const tag = pick(['v:shape', 'v:rect', 'v:roundrect', 'v:oval']);
  const attrs = attr('id', opt(pick(['_x0000_s1026', 'Caixa 1', 'a&amp;b&quot;c', 'Text Box 3', '']), 3)) + attr('o:spid', opt(pick(['_x0000_s1026', '_x0000_s2', 'x', '']), 3))
    + attr('style', decls.join(';'))
    + attr('filled', opt(pick(['f', 'false', 't', '', 'true']), 3)) + attr('stroked', opt(pick(['f', 'false', 't', '']), 3))
    + attr('fillcolor', opt(pick(VCOLORS), 3)) + attr('strokecolor', opt(pick(VCOLORS), 3)) + attr('strokeweight', opt(pick(['1pt', '2', '.5pt', '3t', '', '1e3', '2in', 'x', '12px']), 2))
    + attr('arcsize', opt(pick(['.25', '5461f', 'x', '', '0', '99999999999999999999']), 2)) + attr('alt', opt(pick(['texto', 'a&quot;b&lt;c', 'x &amp; y', '&lt;/wp:docPr&gt;']), 2)) + attr('o:title', opt(pick(['t', 'a&amp;b', '']), 2));
  const kids = `<v:fill${attr('on', opt(pick(['f', 't', 'false']), 3))}${attr('color', opt(pick(VCOLORS), 3))}${attr('color2', opt(pick(VCOLORS), 3))}${attr('opacity', opt(pick(['.5', '32768f', 'x', '', '1', '-.5', 'f', '1e5']), 3))}${attr('type', opt(pick(['gradient', 'gradientRadial', 'solid', 'frame'])))}${attr('angle', opt(pick(INTS)))}/>`
    + `<v:stroke${attr('on', opt(pick(['f', 't']), 4))}${attr('color', opt(pick(VCOLORS), 3))}${attr('weight', opt(pick(['1pt', '2']), 3))}${attr('dashstyle', opt(pick(['dash', 'solid', 'a&quot;b', 'x&lt;y', 'a&amp;b', '', 'dot', '&quot;/&gt;&lt;a:x/&gt;&lt;a:y a=&quot;'])))}${attr('linestyle', opt(pick(['single', 'thinThin', 'thinThick', 'thickThin', 'thickBetweenThin', 'constructor', '__proto__', 'toString', 'nope', ''])))}/>`
    + `<v:textbox${attr('style', opt(pick(['layout-flow:vertical;mso-layout-flow-alt:bottom-to-top', 'layout-flow:vertical;mso-layout-flow-alt:top-to-bottom', 'layout-flow:vertical', 'layout-flow:vertical-ideographic', 'mso-fit-shape-to-text:t', 'layout-flow:x;mso-layout-flow-alt:y', ''])))}${attr('inset', opt(pick(['0,0,0,0', '7.2pt,3.6pt', ',,,', ' 1pt , 2pt ', 'x,y', '1in,,2cm,1e3', '0.1in,0.05in,0.1in,0.05in'])))}><w:txbxContent/></v:textbox>`
    + (chance(2) ? `<w10:wrap${attr('type', opt(pick(['square', 'tight', 'through', 'topAndBottom', 'none', ''])))}/>` : '');
  return `<w:pict>${chance(3) ? '<v:shapetype id="_x0000_t202"/>' : ''}<${tag}${attrs}>${kids}</${tag}></w:pict>`;
}

const kinds = [
  () => `<w:drawing>${anchorFragment()}</w:drawing>`,
  () => `<w:drawing>${anchorFragment()}</w:drawing>`,
  () => `<w:drawing><wp:anchor><a:graphic><a:graphicData>${gradSp()}</a:graphicData></a:graphic></wp:anchor></w:drawing>`,
  () => `<w:drawing><wp:anchor><a:graphic><a:graphicData>${gradSp()}</a:graphicData></a:graphic></wp:anchor></w:drawing>`,
  () => `<w:drawing><wp:anchor><a:graphic><a:graphicData>${shadowSp()}</a:graphicData></a:graphic></wp:anchor></w:drawing>`,
  () => `<w:drawing><wp:anchor><a:graphic><a:graphicData>${spPrFragment()}</a:graphicData></a:graphic></wp:anchor></w:drawing>`,
  () => `<pic:pic><pic:spPr>${chance(2) ? fillEl() : ''}${chance(2) ? lnEl() : ''}</pic:spPr></pic:pic>`,
  vmlPict, vmlTextboxPict, vmlTextboxPict, vmlTextboxPict, backgroundFragment, watermarkPict, custGeom, custGeom,
];
const QUIRKS = 1600;
const quirkLines = [];
for (let i = 0; i < QUIRKS; i++) {
  const frag = pick(kinds)();
  const xml = `<w:root>${frag}</w:root>`;
  quirkLines.push(xml);
  record(`quirk/${i}`, [['fragment', xml]]);
}
// vmlColor over strings that cut or double a surrogate pair (the TS result holds a lone
// surrogate; Rust has U+FFFD, the named difference of the record): compared after the mapping
// (`fffd` of the TS result), together with a few non-ASCII hex-like strings for `toUpperCase`
const ASTRAL = ['#😀a', '#12345😀', '#😀😀', '#a😀b', '#ß12', '#ǆ1', '#ﬃ', '#ΐab', '#ŉ12', '#abcdef😀', '😀', 'red 😀', '#\u{FB00}ab'];
const vmlColors = [...new Set([...VCOLORS, ...ASTRAL, '#aBc', ' #ABC ', '#abcdef 12', 'WHITE', 'Window', '#ABCDEFG', '#', '#ab', '#abcd', '#abcde'])];
vmlColors.forEach((v, i) => {
  const [d] = digest((c) => { const r = D.vmlColor(v); if (r === undefined) c.value(undefined, 'x'); else c.str(fffd(r)); });
  cap.set(`vmlcolor/${i}`, 'c', d);
});
const esc = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
const outDir = resolve(here, '../tests/parity');
if (!only) {
  writeFileSync(join(outDir, 'drawing.quirks.txt'), quirkLines.map(esc).join('\n') + '\n');
  writeFileSync(join(outDir, 'drawing.vmlcolors.txt'), vmlColors.map(esc).join('\n') + '\n');
}
cap.meta('corpus', `${docs} docx, ${QUIRKS} quirk cases, ${records} records`);
if (!only && !traceKey) cap.write(join(outDir, 'drawing.capture'));
console.log(`${docs} docx, ${QUIRKS} quirk cases, ${records} records`);
