// Self-test of the R-S3/R-S4 pair (node --test core/crates/uo-pptx/js/selftest-drawops.mjs):
//  1. the recorder run twice gives identical streams (the frozen TS over a sample deck, and a hand-built list);
//  2. a hand-built Slides list (through the generated codec: encode -> decode) painted by `paint-slides.ts` gives the
//     expected ops, item kind by item kind;
//  3. the SAME slide painted by the frozen TS (`paintSlideTo`) and by the interpreter over a hand-written list that is what
//     the producer will emit: equal draw-form streams (the comparison B4 will run at scale);
//  4. the guards: a throw inside a shape range is contained, a hostile index repairs the save stack.
// The frozen TS root is `UO_TS_ROOT` (default the 0.9.2 reference worktree); parts 1a and 3 are skipped without it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { DEFAULT_TS_ROOT, loadSlidesCore, readSample } from './lib/common.mjs';
import { recordTsSlide } from './lib/ts-paint.mjs';
import { recordList } from './lib/rust-paint.mjs';
import { compare, form } from './lib/opstream.mjs';
import { DEFAULT_CAPS } from './lib/record-ctx.mjs';
import { decodeSlidesList, encodeSlidesList, newEncodeState, newTables } from '../../uo-wasm/js/gen/slides.ts';
import { decodeSlotFrame, encodeSlotFrame, newEncodeState as newFrameState, newTables as newFrameTables } from '../../uo-wasm/js/gen/slots.ts';

const root = process.env.UO_TS_ROOT || DEFAULT_TS_ROOT;
const haveTs = existsSync(join(root, 'slides', 'packages', 'slides-core', 'dist', 'render.js'));

// ---------------------------------------------------------------------------------------------- builders
const view = { ox: 0, oy: 0, scale: 1, sw: 1280, sh: 720 };
const r4 = (x, y, w, h) => ({ x, y, w, h });
const mkList = (over = {}) => ({ view, items: [], paths: [], stops: [], dashes: [], texts: '', gradients: [], patterns: [], runs: [], effects: [], slots: [], ...over });
const css = (v) => ({ kind: 'css', v });
const rectPath = (x, y, w, h) => ({ op: 'rect', x, y, w, h });
const FACE = { family: 'Calibri', bold: false, italic: false };

/** encode -> decode through the generated codec; `tables` is the encoder's tables object (faces, media, ...) */
function roundtrip(list, enc = newTables()) {
  const state = newEncodeState(1, enc);
  const bytes = encodeSlidesList(list, state);
  const dec = newTables();
  return { list: decodeSlidesList(bytes, dec), tables: dec, bytes };
}

const paint = (list, tables, opts = {}) => recordList({ list, tables, width: 1280, height: 720, ...opts });
const draw = (r) => form(r.text);

// ---------------------------------------------------------------------------------------------- 1. determinism
test('the recorder run twice over the frozen TS gives identical streams', { skip: !haveTs }, async () => {
  const core = await loadSlidesCore(root);
  const pkg = core.PptxPackage.load(readSample('gaps-smoke.pptx'));
  const pres = core.readPresentation(pkg);
  const slide = core.readSlide(pkg, pres.slides[0].target);
  const a = await recordTsSlide({ tsRoot: root, slide, pres, scale: 1280 / pres.slideW });
  const b = await recordTsSlide({ tsRoot: root, slide, pres, scale: 1280 / pres.slideW });
  assert.ok(a.lines > 100 && !a.error);
  assert.equal(a.text, b.text);
  assert.equal(a.measures.join('\n'), b.measures.join('\n'));
});

test('the recorder run twice over a hand-built list gives identical streams', () => {
  const l = mkList({ items: [{ op: 'fillStyle', v: css('#123456') }, { op: 'fillRect', ...r4(1, 2, 3, 4) }] });
  const { list, tables } = roundtrip(l);
  const a = paint(list, tables), b = paint(list, tables);
  assert.equal(a.text, b.text);
  assert.deepEqual(draw(a), ['fillRect 1 2 4 2 4 6 1 6 | fs="#123456" alpha=1 comp=source-over']);
});

// ---------------------------------------------------------------------------------------------- 2. item kinds
test('state, transforms, registers and the CTM operand of the off-screen shadow idiom', () => {
  const enc = newTables();
  const l = mkList({
    items: [
      { op: 'translate', x: 10, y: 20 }, { op: 'scale', x: 2, y: 2 }, { op: 'captureCtm', v: 1 },
      { op: 'shadowColor', v: 'rgba(0,0,0,0.5)' }, { op: 'shadowBlur', v: 4 },
      { op: 'shadowOffset', x: { add: 3, k: 20000, axis: 'a' }, y: { add: -2, k: 20000, axis: 'b' } },
      { op: 'translate', x: -20000, y: 0 },
      { op: 'fillStyle', v: css('#000') }, { op: 'fillRect', ...r4(0, 0, 5, 5) },
      { op: 'resetTransform' }, { op: 'transformByInverseOf', v: 1 }, { op: 'transformUnshape', v: 1 },
      { op: 'globalAlpha', v: 0.5 }, { op: 'globalAlphaMul', v: 0.5 },
      { op: 'strokeStyle', v: css('#f00') }, { op: 'lineWidth', v: 2 }, { op: 'lineDash', start: 0, len: 2 },
      { op: 'beginPath' }, { op: 'appendPath', start: 0, len: 3 }, { op: 'stroke' },
    ],
    dashes: [4, 2],
    paths: [{ op: 'moveTo', x: 0, y: 0 }, { op: 'lineTo', x: 10, y: 0 }, { op: 'close' }],
  });
  const { list, tables } = roundtrip(l, enc);
  const r = paint(list, tables);
  assert.equal(r.error, undefined);
  const d = draw(r);
  // the shadow offset is add + m.a * K: 3 + 2 * 20000 and -2 + 0 * 20000 (the CTM was 2 0 0 2 10 20)
  assert.ok(r.text.includes('set shadowOffsetX 40003'), r.text);
  assert.ok(r.text.includes('set shadowOffsetY -2'));
  assert.ok(d.some((x) => x.startsWith('fillRect ') && x.includes('shadow="rgba(0,0,0,0.5)",4,40003,-2')));
  // inverse of the captured matrix, then unshape (the inverse of the current times the register): the CTM is the register
  assert.ok(d.includes('ctm 2 0 0 2 10 20'));
  assert.ok(d.some((x) => x.startsWith('stroke |') && x.includes('alpha=0.25') && x.includes('dash=8,4@0')));
});

test('gradients, conic, patterns (bitmap and image source) and the pattern transform', () => {
  const enc = newTables();
  enc.media.push({ part: 'ppt/media/a.png', ext: 'png', byteLen: 4, natural: { w: 8, h: 8 } });
  enc.bitmaps.push({ w: 2, h: 2, data: 0 });
  const l = mkList({
    stops: [{ offset: 0, color: '#fff' }, { offset: 1, color: '#000' }],
    gradients: [{ g0: 0, g1: 0, g2: 100, g3: 0, g4: 0, g5: 0, stops: { start: 0, len: 2 } }, { g0: 5, g1: 5, g2: 1, g3: 5, g4: 5, g5: 9, stops: { start: 0, len: 2 } }, { g0: 1.5, g1: 50, g2: 50, g3: 0, g4: 0, g5: 0, stops: { start: 0, len: 2 } }],
    patterns: [{ src: { kind: 'bitmap', v: 0 }, repeat: 'repeat', transform: { a: 2, b: 0, c: 0, d: 2, e: 7, f: 9 } }, { src: { kind: 'image', v: { kind: 'media', v: 0 } }, repeat: 'no-repeat' }],
    items: [
      { op: 'translate', x: 100, y: 0 },
      { op: 'fillStyle', v: { kind: 'linear', v: 0 } }, { op: 'fillRect', ...r4(0, 0, 10, 10) },
      { op: 'fillStyle', v: { kind: 'radial', v: 1 } }, { op: 'fillRect', ...r4(0, 0, 10, 10) },
      { op: 'fillStyle', v: { kind: 'conic', v: 2 } }, { op: 'fillRect', ...r4(0, 0, 10, 10) },
      { op: 'fillStyle', v: { kind: 'pattern', v: 0 } }, { op: 'fillRect', ...r4(0, 0, 10, 10) },
      { op: 'fillStyle', v: { kind: 'pattern', v: 1 } }, { op: 'fillRect', ...r4(0, 0, 10, 10) },
    ],
  });
  const { list, tables } = roundtrip(l, enc);
  const rgba = new Uint8Array(16).fill(7);
  const r = paint(list, tables, { media: () => new Uint8Array([1, 2, 3, 4]), bitmap: () => rgba });
  assert.equal(r.error, undefined);
  const d = draw(r).filter((x) => x.startsWith('fillRect'));
  assert.equal(d.length, 5);
  assert.ok(d[0].includes('fs=lin(100,0,200,0|0:#fff;1:#000)'), d[0]);
  assert.ok(d[1].includes('fs=rad(105,5,1,105,5,9|0:#fff;1:#000)'), d[1]);
  assert.ok(d[2].includes('fs=conic(1.5,150,50|0:#fff;1:#000)'), d[2]);
  // device pattern matrix = CTM x pattern transform: translate(100) x [2 0 0 2 7 9] -> e = 107
  assert.ok(/fs=pat\(canvas\(2x2:[0-9a-f]{12}\),repeat,2,0,0,2,107,9\)/.test(d[3]), d[3]);
  assert.ok(/fs=pat\(img\([0-9a-f]{12}\),no-repeat,1,0,0,1,100,0\)/.test(d[4]), d[4]);
});

test('layers: composite, the else block without a layer, the soft-reveal layer rule, keep + drawLayer + release', () => {
  const items = [
    { op: 'layerBegin', layer: 0, fallbackEnd: 7 }, // 0
    { op: 'fillStyle', v: css('#0f0') }, { op: 'fillRect', ...r4(0, 0, 4, 4) }, // 1, 2
    { op: 'layerEnd', layer: 0, then: 'composite' }, // 3
    { op: 'fillStyle', v: css('#f00') }, { op: 'fillRect', ...r4(9, 9, 1, 1) }, // 4, 5: else block
    { op: 'fillRect', ...r4(8, 8, 1, 1) }, // 6: else block
    { op: 'layerBegin', layer: 1, fallbackEnd: 11 }, // 7
    { op: 'fillRect', ...r4(1, 1, 1, 1) }, // 8
    { op: 'layerEnd', layer: 1, then: 'keep' }, // 9
    { op: 'drawLayer', v: 1 }, // 10
    { op: 'releaseLayer', v: 1 }, // 11
  ];
  const { list, tables } = roundtrip(mkList({ items }));
  const withLayers = paint(list, tables);
  const d = draw(withLayers);
  // with layers: the first block runs on canvas 1 and composites; the else block is skipped (jump to 7)
  assert.equal(d.filter((x) => x.startsWith('@cv1 fillRect')).length, 2, d.join('\n'));
  assert.ok(d.some((x) => x.startsWith('drawImage canvas(1280x720:')), 'composite draws the layer');
  assert.ok(!d.some((x) => x.startsWith('fillRect 9 9')), 'else block skipped');
  const noLayers = paint(list, tables, { caps: { ...DEFAULT_CAPS, layers: false } });
  const e = draw(noLayers);
  // without layers: the layered block is skipped and the else block runs; no canvas exists
  assert.ok(e.some((x) => x.startsWith('fillRect 9 9')) && e.some((x) => x.startsWith('fillRect 8 8')), e.join('\n'));
  assert.ok(!e.some((x) => x.includes('@cv')) && noLayers.error === undefined);
});

test('slots: hidden, state with the reveal mask, shape opacity and the soft-reveal layer', () => {
  const slots = [
    { kind: 'paragraph', shape: 0, index: 0, base: r4(0.5, 0.5, 0.2, 0.2), bx: r4(10, 10, 100, 100), baseRotRad: 0 },
    { kind: 'shape', shape: 1, index: 0, base: r4(0.5, 0.5, 0.2, 0.2), bx: r4(10, 10, 100, 100), baseRotRad: 0 },
  ];
  const items = [
    { op: 'slotBegin', v: 0 }, { op: 'fillRect', ...r4(0, 0, 1, 1) }, { op: 'slotEnd' },
    { op: 'slotBegin', v: 1 },
    { op: 'layerBegin', layer: 0, fallbackEnd: 8 },
    { op: 'fillRect', ...r4(10, 10, 100, 100) },
    { op: 'layerEnd', layer: 0, then: 'composite' },
    { op: 'fillRect', ...r4(10, 10, 100, 100) },
    { op: 'slotEnd' },
  ];
  const { list, tables } = roundtrip(mkList({ items, slots }));
  const part = (o) => ({ t: 'state', visible: true, opacity: 1, x: 60 / 1280, y: 60 / 720, w: 0.2, h: 0.2, rot: 0, sx: 1, sy: 1, mx: 0, my: 0, filters: [], ...o });
  const frameOf = (values) => {
    const st = newFrameState(1, newFrameTables());
    return decodeSlotFrame(encodeSlotFrame({ frame: 1, values, replacements: [] }, st), newFrameTables());
  };
  // static: paints as the template (save/restore bracket)
  let r = paint(list, tables);
  assert.equal(r.error, undefined);
  assert.ok(draw(r).includes('save') && draw(r).filter((x) => x.startsWith('fillRect')).length >= 2);
  // hidden paragraph: its items are skipped; shape opacity 0.5 multiplies the alpha
  r = paint(list, tables, { frame: frameOf([{ t: 'hidden' }, { t: 'shape', op: 0.5 }]) });
  let d = draw(r);
  assert.ok(!d.includes('fillRect 0 0 1 0 1 1 0 1 | fs="#000000" alpha=1 comp=source-over'));
  assert.ok(d.some((x) => x.startsWith('fillRect 10 10') && x.includes('alpha=0.5')), d.join('\n'));
  // a state with a wipe at q = 0.5 clips the paragraph; at q = 1 nothing is clipped
  r = paint(list, tables, { frame: frameOf([part({ filters: [{ filter: 'wipe(down)', out: false, p: 0.5 }] }), { t: 'static' }]) });
  d = draw(r);
  assert.ok(d.some((x) => x.startsWith('rect 10 60 110 60 110 110 10 110')), d.join('\n')); // wipe(down), q 0.5: the lower half
  // a soft wipe on the shape slot takes the layer (composite applies destination-in with a gradient on the layer)
  r = paint(list, tables, { frame: frameOf([{ t: 'static' }, { t: 'shape', op: 1, all: part({ filters: [{ filter: 'wipe(down)', out: false, p: 0.5 }] }) }]) });
  d = draw(r);
  assert.ok(d.some((x) => x.startsWith('@cv1 fillRect 10 10 110 10 110 110 10 110') && x.includes('comp=destination-in')), d.join('\n'));
  assert.equal(d.filter((x) => x.startsWith('fillRect 10 10')).length, 0, 'the else block is skipped when the layer is taken');
  // no soft reveal pending: the layer is not taken and the else block paints
  r = paint(list, tables, { frame: frameOf([{ t: 'static' }, { t: 'shape', op: 1 }]) });
  d = draw(r);
  assert.equal(d.filter((x) => x.startsWith('fillRect 10 10')).length, 1);
  assert.ok(!d.some((x) => x.includes('@cv')));
});

test('text: fit by measureText, letter spacing, shadow, outline, decorations', () => {
  const enc = newTables();
  enc.faces.push(FACE, { family: 'Cambria', bold: true, italic: true });
  const texts = 'Hello World';
  const run = (over) => ({
    text: { start: 0, len: 5 }, font: { face: 0, sizePx: 20 }, x: 10, y: 50, laidW: 200, fitAllowed: true, letterSpacingPx: 0, kern: true,
    fill: css('#112233'), fillText: true, ...over,
  });
  const l = mkList({
    texts,
    runs: [run({}), run({ letterSpacingPx: 1.5, fitAllowed: false, font: { face: 1, sizePx: 18 }, shadow: { color: 'rgba(0,0,0,0.3)', blur: 2, dx: 1, dy: 2 }, outline: { color: '#fff', width: 0.75 }, alphaMul: 0.5, underline: { color: '#112233', width: 1.5, y: 52.5 }, strike: { color: '#112233', width: 1.5, y: 44.5 } }), run({ fillText: false, laidW: undefined, fitAllowed: false, text: { start: 6, len: 5 } })],
    items: [{ op: 'text', v: 0 }, { op: 'text', v: 1 }, { op: 'text', v: 2 }],
  });
  const { list, tables } = roundtrip(l, enc);
  const r = paint(list, tables);
  assert.equal(r.error, undefined);
  const t = r.text;
  // run 0: the stub's width of "Hello" at 20 px is not 200: the difference spreads over the 5 characters
  const w = r.measures.find((x) => x.includes('"Hello"'));
  assert.ok(w, 'measureText asked for the run text');
  const width = Number(w.split('\t')[2]);
  const fit = (200 - width) / 5;
  assert.ok(t.includes(`set letterSpacing "${fit}px"`), t.split('\n').filter((x) => x.includes('letterSpacing')).join('|'));
  assert.ok(t.includes('set letterSpacing "0px"'), 'restored');
  assert.ok(t.includes('set fontKerning "normal"') && t.includes('set fontKerning "auto"'));
  // run 1: spacing 1.5 px, a Cambria bold italic font, shadow around fillText, outline stroke, both decorations, alpha x 0.5
  assert.ok(t.includes('set font "italic 700 18px \'Cambria\''), t);
  assert.ok(t.includes('set letterSpacing "1.5px"'));
  assert.ok(t.split('\n').some((x) => x.startsWith('fillText "Hello"') && x.includes('shadow="rgba(0,0,0,0.3)",2,1,2') && x.includes('alpha=0.5')));
  assert.ok(t.split('\n').some((x) => x.startsWith('strokeText "Hello"') && x.includes('join=round') && x.includes('lw=0.75')));
  assert.equal(t.split('\n').filter((x) => x.startsWith('moveTo 10 52.5') || x.startsWith('moveTo 10 44.5')).length, 2);
  // run 2: outline only (no fillText), no laid width
  assert.equal(t.split('\n').filter((x) => x.startsWith('fillText "World"')).length, 0);
});

test('drawImage, derived images, bitmaps, media frames and a missing picture', () => {
  const enc = newTables();
  enc.media.push({ part: 'p/a.png', ext: 'png', byteLen: 4, natural: { w: 8, h: 4 } }, { part: 'p/missing.png', ext: 'png', byteLen: 4 });
  enc.derived.push({ image: 0, fx: { gray: true } });
  enc.bitmaps.push({ w: 1, h: 1, data: 0 });
  const l = mkList({
    items: [
      { op: 'drawImage', src: { kind: 'media', v: 0 }, d: r4(1, 2, 3, 4) },
      { op: 'drawImage', src: { kind: 'media', v: 0 }, s: r4(0, 0, 4, 2), d: r4(5, 6, 7, 8) },
      { op: 'drawImage', src: { kind: 'derived', v: 0 }, d: r4(1, 1, 2, 2) },
      { op: 'drawImage', src: { kind: 'bitmap', v: 0 }, d: r4(0, 0, 1, 1) },
      { op: 'drawImage', src: { kind: 'media', v: 1 }, d: r4(0, 0, 9, 9) },
      { op: 'mediaFrame', shape: 3, d: r4(1, 2, 3, 4) },
    ],
  });
  const { list, tables } = roundtrip(l, enc);
  const frameImg = { width: 2, height: 2, naturalWidth: 2, naturalHeight: 2, __h: 'abcabcabcabc' };
  const r = paint(list, tables, {
    media: (i) => (i === 0 ? new Uint8Array([9, 9, 9, 9]) : null), bitmap: () => new Uint8Array([1, 2, 3, 255]),
    mediaFrame: (s) => (s === 3 ? { image: frameImg, alpha: 0.4 } : null),
  });
  assert.equal(r.error, undefined);
  const d = draw(r);
  const imgs = d.filter((x) => x.startsWith('drawImage'));
  assert.equal(imgs.length, 5, imgs.join('\n')); // the missing picture is skipped
  assert.ok(imgs[0].startsWith('drawImage img(') && imgs[0].includes(' 1 2 4 2 4 6 1 6 '));
  assert.ok(imgs[1].includes(' s=0,0,4,2 '));
  assert.ok(imgs[2].startsWith('drawImage canvas(8x4:'), 'the recoloured picture is a canvas');
  assert.ok(d.some((x) => x.includes('getImageData 0 0 8 4')), 'fxImage read the pixels');
  assert.ok(imgs[3].startsWith('drawImage canvas(1x1:'));
  assert.ok(imgs[4].startsWith('drawImage img(abcabcabcabc)') && imgs[4].includes('alpha=0.4'));
  // the caches are bounded and cleared
  assert.deepEqual(r.painter.cacheSizes(), { derived: 1, bitmaps: 1 });
  r.painter.clear();
  assert.deepEqual(r.painter.cacheSizes(), { derived: 0, bitmaps: 0 });
});

test('effects: bevel (moved applyBevel), the raster-op rectangle and the WordArt warp', () => {
  const enc = newTables();
  enc.faces.push(FACE);
  enc.bitmaps.push({ w: 1, h: 1, data: 0 });
  const light = {
    lightTable: () => new Uint8Array(4), lightFace: (t, out) => out.fill(0.5), lightAt: (t, dir, th, ph, out) => out.fill(0.25), bevelPhi: () => 30,
  };
  const l = mkList({
    paths: [{ op: 'rect', x: 100, y: 100, w: 80, h: 40 }],
    effects: [
      { kind: 'bevel', path: { start: 0, len: 1 }, rule: 'nonzero', bx: r4(100, 100, 80, 40), w: 6, h: 6, scale: 1, rig: 'threePt' },
      { kind: 'metafileRop', rop: 0x5a0049, rect: r4(10, 20, 30, 40), operand: 0 },
      {
        kind: 'warp', prst: 'textArchUp', fromWordArt: false, bx: r4(50, 50, 400, 200), pxPerPt: 1.5,
        shadow: { color: 'rgba(0,0,0,0.4)', blur: 3, dx: 2, dy: 2 },
        lines: [{ baseline: 40, sizePt: 40, align: 'ctr', runs: [{ text: 'ARC', font: { face: 0, sizePt: 40 }, x: 0, color: '#336699' }] }],
      },
    ],
    items: [{ op: 'effect', v: 0 }, { op: 'effect', v: 1 }, { op: 'effect', v: 2 }],
  });
  const { list, tables } = roundtrip(l, enc);
  const r = paint(list, tables, { light, bitmap: () => new Uint8Array([255, 0, 128, 255]), patternRows: () => undefined });
  assert.equal(r.error, undefined);
  const t = r.text;
  assert.ok(t.includes('getImageData 99 99 82 42'), 'bevel reads the device rect');
  assert.ok(t.includes('@cv1 rect 1 1 81 1 81 41 1 41'), 'the silhouette mask is built on a scratch canvas, offset by the device rect');
  assert.ok(t.includes('putImageData 99 99 82 42'), 'bevel writes it back');
  assert.ok(t.includes('getImageData 10 20 30 40') && t.includes('putImageData 10 20 30 40'), 'PATINVERT is the exact XOR');
  assert.ok(t.includes('@cv') && t.includes('fillText "ARC"') && t.includes('drawImage canvas('), 'the warp rasters the line and composites with the shadow');
  assert.ok(t.includes('shadow="rgba(0,0,0,0.4)",3,2,2'));
  // without the light tables the bevel is skipped (nothing read)
  const nb = paint(list, tables, { bitmap: () => new Uint8Array([255, 0, 128, 255]), env: { light: undefined } });
  assert.ok(!nb.text.includes('getImageData 99 99'));
});

// ---------------------------------------------------------------------------------------------- 3. TS vs interpreter
test('frozen TS paintSlideTo vs the interpreter over the list the producer will emit: equal draw-form streams', { skip: !haveTs }, async () => {
  const pres = { slideW: 9144000, slideH: 5143500 };
  const slide = {
    bg: { type: 'solid', color: '#FFFFFF' },
    shapes: [
      { id: '1', name: 'r', xfrm: { x: 914400, y: 914400, cx: 1828800, cy: 914400 }, geom: 'rect', fill: { type: 'solid', color: '#336699' }, lineColor: '#000000', lineWidth: 12700 },
      { id: '2', name: 't', xfrm: { x: 914400, y: 2914400, cx: 3828800, cy: 914400 }, txBody: { paragraphs: [{ runs: [{ text: 'Hello', sz: 20, color: '#112233', underline: true }] }] } },
    ],
  };
  const scale = 1280 / pres.slideW;
  const ts = await recordTsSlide({ tsRoot: root, slide, pres, scale });
  assert.equal(ts.error, undefined);
  // what the producer emits for it: the TS calls as list items (geometry precomputed with the TS arithmetic)
  const rectP = rectPath(128, 128, 256, 128);
  const underlineTw = Number(/lineTo ([\d.]+) 453.5/.exec(ts.text)[1]) - 140.8;
  const enc = newTables();
  enc.faces.push({ family: 'Calibri', bold: false, italic: false });
  const l = mkList({
    paths: [rectPath(0, 0, 1280, 720), rectP],
    texts: 'Hello',
    runs: [{ text: { start: 0, len: 5 }, font: { face: 0, sizePx: 35.55555555555556 }, x: 140.8, y: Number(/fillText "Hello" 140.8 ([\d.]+)/.exec(ts.text)[1]), laidW: underlineTw, fitAllowed: true, letterSpacingPx: 0, kern: false, fill: css('#112233'), fillText: true, underline: { color: '#112233', width: 2.2222222222222223, y: 453.5 } }],
    items: [
      { op: 'fillStyle', v: css('#FFFFFF') }, { op: 'fillRect', ...r4(0, 0, 1280, 720) },
      { op: 'save' }, { op: 'beginPath' }, { op: 'appendPath', start: 0, len: 1 }, { op: 'clip', v: 'nonzero' },
      { op: 'save' }, { op: 'save' }, { op: 'save' },
      { op: 'beginPath' }, { op: 'appendPath', start: 1, len: 1 }, { op: 'fillStyle', v: css('#336699') }, { op: 'fill', v: 'nonzero' },
      { op: 'restore' },
      { op: 'beginPath' }, { op: 'appendPath', start: 1, len: 1 }, { op: 'strokeStyle', v: css('#000000') }, { op: 'lineWidth', v: 1.7777777777777777 }, { op: 'stroke' },
      { op: 'restore' }, { op: 'restore' },
      { op: 'save' }, { op: 'save' }, { op: 'restore' }, { op: 'save' },
      { op: 'text', v: 0 },
      { op: 'restore' }, { op: 'restore' }, { op: 'restore' },
    ],
  });
  const { list, tables } = roundtrip(l, enc);
  const rs = paint(list, tables);
  assert.equal(rs.error, undefined);
  const cmp = compare(ts.text, rs.text, { which: 'draw', eps: 1e-9 });
  assert.ok(cmp.equal, `first difference at ${cmp.index}:\n TS:   ${cmp.a}\n list: ${cmp.b}`);
  // the loose form (no save/restore/ctm) agrees too
  assert.ok(compare(ts.text, rs.text, { which: 'loose', eps: 1e-9 }).equal);
});

// ---------------------------------------------------------------------------------------------- 4. guards
test('a throw inside a shape range is contained; a throw outside repairs the saves and propagates', () => {
  const bad = [{ op: 'beginPath' }, { op: 'appendPath', start: 0, len: 1 }, { op: 'fill', v: 'nonzero' }];
  const l = mkList({
    paths: [{ op: 'arc', cx: 5, cy: 5, r: -3, a0: 0, a1: 1, ccw: false }],
    items: [
      { op: 'save' }, // the slide clip
      { op: 'save' }, { op: 'save' }, ...bad, { op: 'restore' }, { op: 'restore' }, // shape 1: throws (negative radius)
      { op: 'save' }, { op: 'fillRect', ...r4(1, 1, 2, 2) }, { op: 'restore' }, // shape 2: paints
      { op: 'restore' },
    ],
  });
  const { list, tables } = roundtrip(l);
  const r = paint(list, tables);
  assert.equal(r.error, undefined);
  const d = draw(r);
  assert.ok(d.some((x) => x.startsWith('fillRect 1 1 3 1 3 3 1 3')), 'the next shape still paints');
  // the contained shape left the save stack balanced: as many restores as saves
  const saves = r.text.split('\n').filter((x) => x === 'save').length, restores = r.text.split('\n').filter((x) => x === 'restore').length;
  assert.equal(saves, restores);
  // a hostile index outside any range: the saves are undone and the error reaches the caller
  const h = roundtrip(mkList({ items: [{ op: 'save' }, { op: 'translate', x: 1, y: 1 }, { op: 'text', v: 7 }] }));
  const rh = paint(h.list, h.tables);
  assert.match(rh.error, /run 7 out of range/);
  const hs = rh.text.split('\n');
  assert.equal(hs.filter((x) => x === 'save').length, hs.filter((x) => x === 'restore').length);
});

// ---------------------------------------------------------------------------------------------- 5. caches (M5)
test('the caches are capped and the layer pool does not grow across paints', async () => {
  const { SlidesPainter, MAX_BITMAP_CACHE } = await import('../../uo-wasm/js/paint-slides.ts');
  const { createRecorder } = await import('./lib/record-ctx.mjs');
  const enc = newTables();
  const n = MAX_BITMAP_CACHE + 30;
  const items = [];
  for (let i = 0; i < n; i++) { enc.bitmaps.push({ w: 1, h: 1, data: i }); items.push({ op: 'drawImage', src: { kind: 'bitmap', v: i }, d: r4(0, 0, 1, 1) }); }
  const a = roundtrip(mkList({ items }), enc);
  const rec = createRecorder({ width: 64, height: 64 });
  const painter = new SlidesPainter({ media: () => null, bitmap: () => new Uint8Array([1, 2, 3, 4]) });
  rec.install();
  try {
    painter.paint(rec.ctx, a.list, a.tables);
    assert.equal(painter.cacheSizes().bitmaps, MAX_BITMAP_CACHE);
    // a layered list painted many times on one canvas reuses the pooled layer
    const layered = roundtrip(mkList({ items: [{ op: 'layerBegin', layer: 0, fallbackEnd: 3 }, { op: 'fillRect', ...r4(0, 0, 4, 4) }, { op: 'layerEnd', layer: 0, then: 'composite' }] }));
    const before = rec.root.canvases;
    for (let i = 0; i < 60; i++) painter.paint(rec.ctx, layered.list, layered.tables);
    assert.equal(rec.root.canvases - before, 1, 'one layer canvas serves every paint');
  } finally { rec.uninstall(); }
});
