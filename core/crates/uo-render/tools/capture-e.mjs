// Parity capture of frontier E of uo-render (the canvas encoder): the frozen TS `paintItems`
// (canvas.ts, dist) runs over generated glyph runs and shapes with a RECORDING canvas context, and
// the log of every drawing call it makes is digested. tests/e_*.rs build the same items (the testkit
// `Rng`, the same pools), encode them with `encode_page_for_canvas`, decode the message with the
// generated codec and replay the painter's decisions from the hints alone (a transcription of the
// part of canvas.ts that reads the hints, `tests/e_support/shell.rs`: the contract frontier D
// implements in TS), logging the same calls. Equal digests = the hints carry everything the canvas
// asked a font or a geometry function for (§3.3).
//
//   node tools/capture-e.mjs --ts-root <checkout of main>     (or UO_TS_ROOT)
//
// Writes tests/captures/e_hints.capture. Taken in WSL like frontier G's (gradientSamples uses `**`):
//   GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2 node tools/capture-e.mjs --ts-root /mnt/d/...
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { CaptureWriter, Rng, Stream, tsRoot } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : undefined; };
const outDir = opt('--out-dir') ?? join(here, '..', 'tests', 'captures');
const root = tsRoot(argv);
const dl = (p) => join(root, 'write/packages/docx-layout/dist', p);
const canvas = await import(pathToFileURL(dl('canvas.js')).href);
const textfx = await import(pathToFileURL(dl('textfx.js')).href);
const geometry = await import(pathToFileURL(dl('geometry.js')).href);
const { SfntFont } = await import(pathToFileURL(dl('typography/font/sfnt.js')).href);

const canon = (v) => (v !== v ? NaN : v);
const f = (s, v) => s.f64(canon(v));

const GOOD = [0, 1, 2, 5, 10, 16.5, 50, 100, 150.25, 240, 720, 1440, 3000, 5760, 11906, 0.5, 0.25, 7, 33.333];
const BAD = [-0, -1, -5.5, 1e-7, 1e9, 1e15, NaN, Infinity, -Infinity, 1e308, -1e308];
const pick = (r, a) => a[r.below(a.length)];
const num = (r, bad = 12) => (r.below(100) < bad ? BAD[r.below(BAD.length)] : GOOD[r.below(GOOD.length)]);
const bool = (r) => r.below(2) === 1;
const COLORS = ['FF0000', '00FF00', '0000FF', 'FFFFFF', '000000', '123456', 'abcdef', 'ABCDEF', '', 'zz', '12', '-1f0a0b', ' 5a5a5a', '0x1234', 'ff', '+f+f+f', '7f7f7f', 'ffffff00', 'FF00', '8080GG'];
const VALID = ['000000', 'FF0000', '808080', '12AB34', 'FFFFFF', '00B050'];
const POS = [0, 1, 0.5, 0.25, 0.75, -0.5, 1.5, NaN, 0.3333, 0.999];

// ── the recording context ───────────────────────────────────────────────────────────────────────

const LOGGED = new Set(['fillText', 'strokeText', 'beginPath', 'moveTo', 'lineTo', 'quadraticCurveTo', 'bezierCurveTo', 'closePath', 'fill', 'stroke', 'translate', 'transform', 'save', 'restore', 'setLineDash', 'createLinearGradient', 'createRadialGradient', 'addColorStop']);

function recorder(kerning) {
  const log = [];
  const rec = (name) => (...a) => { log.push([name, a]); };
  const grad = () => ({ addColorStop: (...b) => { log.push(['addColorStop', b]); } });
  const ctx = {
    getTransform: () => ({ a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 }),
    createLinearGradient: (...a) => { log.push(['createLinearGradient', a]); return grad(); },
    createRadialGradient: (...a) => { log.push(['createRadialGradient', a]); return grad(); },
  };
  for (const n of LOGGED) if (!(n in ctx)) ctx[n] = rec(n);
  // the context calls the painter makes that the digest leaves out
  for (const n of ['rect', 'clip', 'rotate', 'scale', 'drawImage', 'fillRect', 'setTransform', 'createPattern', 'measureText']) ctx[n] = () => undefined;
  if (kerning) ctx.fontKerning = 'auto';
  return { ctx, log };
}

const ENV = { fontOf: () => ({ family: '"F"', weight: 400, italic: false }), image: () => null, color: (h) => `#${h}` };

function arg(s, v) {
  if (typeof v === 'number') { s.tag('n'); f(s, v); } else if (typeof v === 'string') {
    const cps = [...v].map((c) => c.codePointAt(0));
    s.tag('s'); s.u32(cps.length); for (const c of cps) s.u32(c);
  } else if (Array.isArray(v)) { s.tag('a'); s.u32(v.length); for (const e of v) arg(s, e); } else s.tag('?');
}
function logOut(s, log) {
  s.u32(log.length);
  for (const [name, a] of log) { s.str(name); s.u32(a.length); for (const v of a) arg(s, v); }
}

// ── glyph runs ──────────────────────────────────────────────────────────────────────────────────

const FONT_FILES = ['liberationserif-regular', 'carlito-regular', 'liberationmono-bold', 'caladea-italic'];
const GLYPH_CPS = [...'Hello, World! fi 123 gjpqy'].map((c) => c.codePointAt(0)).concat([0xf020, 0xf041, 0xf0b7, 0xf0ff, 0x200c, 0x4e00, 0x20ac, 0x0416, 0x03b1, 0x2022, 0x05d0, 0x0300, 0x1f600, 0x00e9]);
const JITTER = [0, 0, 0, 0.2, -0.3, 0.26, 1, 700, 0.25, NaN];

function genGlyphRun(r, fonts, maxGlyphs = 14) {
  const font = fonts[r.below(fonts.length)];
  const sizePt = pick(r, [8, 11, 14.5, 36, 0.5, 0]);
  const kind = r.below(8);
  const n = 1 + r.below(maxGlyphs);
  const upem = font.metrics.unitsPerEm || 1000;
  let x = pick(r, [0, 1440, -200, 5000.5]);
  const glyphs = [];
  for (let i = 0; i < n; i++) {
    const cp = pick(r, GLYPH_CPS);
    const g = { cp, x };
    if (r.below(7) === 0) g.gid = pick(r, [0, 1, 36, 70, 499, 3]);
    glyphs.push(g);
    const adv = (font.advanceWidth(font.glyphId(cp)) * sizePt * 20) / upem;
    x = x + adv + pick(r, JITTER);
  }
  const y = pick(r, [0, 240, 1440.5]);
  const color = pick(r, VALID);
  const it = { t: 'glyphs', face: { font }, sizePt, color, y, glyphs };
  if (kind === 0) it.scaleX = pick(r, [2, 0.5]);
  if (kind === 1) it.synthItalic = true;
  if (kind === 2) it.scaleX = 1;
  if (r.below(5) === 0) it.synthBold = true;
  return it;
}

function captureGlyphs(c, fonts) {
  for (const kerning of [true, false]) {
    const r = new Rng(7001);
    let batched = 0, quads = 0, strings = 0;
    const s = new Stream();
    for (let i = 0; i < 1500; i++) {
      const it = genGlyphRun(r, fonts);
      const { ctx, log } = recorder(kerning);
      canvas.paintItems(ctx, [it], ENV);
      for (const [name, a] of log) {
        if (name === 'fillText') { strings++; if ([...a[0]].length > 1) batched++; }
        if (name === 'quadraticCurveTo') quads++;
      }
      logOut(s, log);
    }
    c.set(kerning ? 'glyphs.kern' : 'glyphs.nokern', 'v', `${s.items}:${s.finish().short}`);
    c.set(kerning ? 'glyphs.kern' : 'glyphs.nokern', 'stats', `strings=${strings};batched=${batched};quads=${quads}`);
  }
}

// ── fx runs: the box and the gradient geometries ────────────────────────────────────────────────

const PATHS = [undefined, undefined, 'circle', 'path', 'rect'];
function genFxGrad(r) {
  const n = 2 + r.below(2);
  const stops = [];
  for (let i = 0; i < n; i++) stops.push({ pos: pick(r, POS), hex: pick(r, VALID), alpha: pick(r, [1, 0.5, 0]) });
  const g = { kind: 'grad', stops, angleDeg: pick(r, [0, 45, 90, 135, 270, 30.5]), scaled: bool(r) };
  const p = pick(r, PATHS);
  if (p !== undefined) g.path = p;
  if (bool(r)) g.fillTo = { l: pick(r, [0, 0.5, 0.25]), t: pick(r, [0, 0.5, 0.25]), r: pick(r, [0, 0.5, 1]), b: pick(r, [0.5, 0.25, 0]) };
  return g;
}
function genFxFill(r) {
  switch (r.below(4)) {
    case 0: return { kind: 'none' };
    case 1: return { kind: 'solid', hex: pick(r, VALID), alpha: pick(r, [1, 0.5]) };
    default: return genFxGrad(r);
  }
}

function captureFxRun(c, fonts) {
  const r = new Rng(7002);
  const s = new Stream();
  let grads = 0;
  for (let i = 0; i < 800; i++) {
    const it = genGlyphRun(r, fonts, 8);
    const fx = {};
    if (bool(r)) fx.fill = genFxFill(r);
    if (bool(r)) fx.outline = { widthPt: pick(r, [0.5, 1, 2.25]), fill: genFxFill(r), cap: 'flat', join: 'miter' };
    it.fx = fx;
    const box = textfx.runBox(it);
    f(s, box.x); f(s, box.y); f(s, box.w); f(s, box.h);
    const geomOf = (fl) => {
      if (!fl || fl.kind !== 'grad') { s.tag('N'); return; }
      grads++;
      const g = geometry.gradientGeometry(textfx.gradOf(fl), box, { x0: box.x, y0: box.y, x1: box.x + box.w, y1: box.y + box.h });
      s.tag(g.kind === 'axial' ? 'A' : 'R');
      f(s, g.x0); f(s, g.y0);
      if (g.kind === 'axial') { f(s, g.x1); f(s, g.y1); } else f(s, g.r);
    };
    geomOf(fx.fill);
    geomOf(fx.outline?.fill);
  }
  c.set('fxrun', 'v', `${s.items}:${s.finish().short}`);
  c.set('fxrun', 'stats', `grads=${grads}`);
}

// ── shapes ──────────────────────────────────────────────────────────────────────────────────────

const GEOMS = ['rect', 'ellipse', 'roundRect', 'triangle', 'line', 'straightConnector1', 'custom', 'rightArrow', 'bentConnector3', 'star5', 'unknownShape', 'frame', 'custom'];
const SWEEPS = [90, -90, 180, 270, 360, 45, 30.5, 0];
function genCust(r) {
  const paths = [];
  const np = 1 + r.below(3);
  for (let p = 0; p < np; p++) {
    const w = pick(r, [100, 1, 21600, 3.5, 0]);
    const h = pick(r, [100, 1, 21600, 3.5, 0]);
    const fill = bool(r), stroke = bool(r);
    const cmds = [];
    const nc = 2 + r.below(7);
    for (let i = 0; i < nc; i++) {
      switch (r.below(8)) {
        case 0: case 6: cmds.push({ op: 'M', x: num(r, 3), y: num(r, 3) }); break;
        case 1: case 7: cmds.push({ op: 'L', x: num(r, 3), y: num(r, 3) }); break;
        case 2: { const k = r.below(5); const pts = []; for (let j = 0; j < k; j++) pts.push([num(r, 3), num(r, 3)]); cmds.push({ op: 'C', pts }); break; }
        case 3: { const k = r.below(4); const pts = []; for (let j = 0; j < k; j++) pts.push([num(r, 3), num(r, 3)]); cmds.push({ op: 'Q', pts }); break; }
        case 4: cmds.push({ op: 'A', wR: num(r, 3), hR: num(r, 3), st: num(r, 3), sw: pick(r, SWEEPS) }); break;
        default: cmds.push({ op: 'Z' });
      }
    }
    paths.push({ w, h, fill, stroke, cmds });
  }
  return paths;
}
function genAdj(r) {
  switch (r.below(5)) {
    case 0: return undefined;
    case 1: return { adj: num(r, 3) };
    case 2: return { adj1: num(r, 3), adj2: num(r, 3) };
    case 3: return { vf: num(r, 3), adj: num(r, 3) };
    default: return { adj1: num(r, 3), adj2: num(r, 3), adj: num(r, 3) };
  }
}
const GPATHS = [undefined, undefined, 'circle', 'path'];
function genGrad(r) {
  const n = 2 + r.below(2);
  const stops = [];
  for (let i = 0; i < n; i++) stops.push({ pos: pick(r, POS), color: pick(r, COLORS) });
  const g = { stops, angleDeg: pick(r, [0, 45, 90, 135, 270, 30.5]), scaled: bool(r), rotWithShape: bool(r) };
  const p = pick(r, GPATHS);
  if (p !== undefined) g.path = p;
  if (bool(r)) g.fillTo = { l: pick(r, [0, 0.5, 0.25]), t: pick(r, [0, 0.5, 0.25]), r: pick(r, [0, 0.5, 1]), b: pick(r, [0.5, 0.25, 0]) };
  return g;
}

function genShape(r) {
  const geom = pick(r, GEOMS);
  const style = { geom, flipH: bool(r), flipV: bool(r) };
  const adj = genAdj(r);
  if (adj) style.adj = adj;
  const rot = pick(r, [undefined, undefined, 0, 15, 90, -30, 45.5]);
  if (rot !== undefined) style.rot = rot;
  if (geom === 'custom') style.cust = genCust(r);
  const x = num(r, 4), y = num(r, 4), w = num(r, 4), h = num(r, 4);
  const it = { t: 'shape', x, y, w, h, style };
  const fill = pick(r, [undefined, 'FF0000', 'zz', '00B050']);
  if (fill !== undefined) it.fill = fill;
  if (r.below(3) !== 0) {
    it.stroke = { color: pick(r, ['000000', 'FF0000', 'qq', '12AB34']), widthPt: pick(r, [0.5, 1, 3, 0]) };
    if (bool(r)) it.stroke.dash = pick(r, ['dash', '']);
  }
  if (r.below(3) === 0) it.grad = genGrad(r);
  if (r.below(4) === 0) it.shadow = { dx: num(r, 3), dy: num(r, 3), color: pick(r, ['808080', 'zz']), alpha: pick(r, [1, 0.5]) };
  if (['line', 'straightConnector1', 'bentConnector3'].includes(geom) && bool(r)) it.line = { x1: num(r, 3), y1: num(r, 3), x2: num(r, 3), y2: num(r, 3) };
  if (r.below(5) === 0) it.fx = {};
  return it;
}

function captureShapes(c) {
  const r = new Rng(7003);
  const s = new Stream();
  const sf = new Stream();
  let painted = 0, grads = 0, fxs = 0, parts = 0;
  for (let i = 0; i < 2500; i++) {
    const it = genShape(r);
    // an fx shape is painted by the canvas through the local item (canvas.ts:430)
    const effects = !!it.fx && it.style.geom !== 'line';
    const probe = effects ? { ...it, x: 0, y: 0, style: { ...it.style, rot: 0 }, fx: undefined, shadow: undefined } : it;
    const { ctx, log } = recorder(true);
    canvas.paintItems(ctx, [probe], ENV);
    if (log.length > 2) painted++;
    if (log.some(([n]) => n === 'createLinearGradient' || n === 'createRadialGradient')) grads++;
    parts += log.filter(([n]) => n === 'beginPath').length;
    if (effects) { fxs++; logOut(sf, log); } else logOut(s, log);
  }
  c.set('shapes', 'v', `${s.items}:${s.finish().short}`);
  c.set('shapes', 'stats', `painted=${painted};grads=${grads};fx=${fxs};begin=${parts}`);
  c.set('shapes.fx', 'v', `${sf.items}:${sf.finish().short}`);
}

// ── main ────────────────────────────────────────────────────────────────────────────────────────

const fonts = FONT_FILES.map((n) => new SfntFont(new Uint8Array(readFileSync(join(root, 'shared/fonts/files', `${n}.ttf`)))));
const t0 = Date.now();
const c = new CaptureWriter(['Parity capture of frontier E of uo-render (canvas hints): written by core/crates/uo-render/tools/capture-e.mjs, replayed by tests/e_hints.rs.']);
captureGlyphs(c, fonts);
captureFxRun(c, fonts);
captureShapes(c);
c.write(join(outDir, 'e_hints.capture'));
console.log(`e_hints.capture: ${c.records.size} records, ${Date.now() - t0} ms`);
