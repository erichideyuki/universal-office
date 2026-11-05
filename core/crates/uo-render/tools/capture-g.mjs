// Parity capture of frontier G of uo-render (geometry and painter helpers): the frozen TS
// `geometry.ts`, `page-fill.ts`, `textfx.ts` and the private helpers of `canvas.ts`, `pdf.ts`,
// `canvas-picture.ts` and `pdf-picture.ts` (extracted from the built `dist` by source text, so the
// real function runs, not a copy) over the corpora below. Writes tests/captures/g_geom.capture,
// g_fill.capture, g_page_fill.capture, g_text_fx.capture and g_picture.capture, replayed by
// tests/g_*.rs, which mirror every generator of this file (the testkit `Rng`, the same pools).
//
//   node tools/capture-g.mjs --ts-root <checkout of main>     (or UO_TS_ROOT)
//
// Options: --out-dir <dir>   write elsewhere (comparisons between platforms)
//          --only <name>     one of geom, fill, page_fill, text_fx, picture
//
// The committed files are taken in WSL (Node v24.15.0 linux-x64, glibc 2.39,
// GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2: the pow reference environment), because the
// gradient ramp uses `**` (gradientSamples: fill, page_fill and the rect gradient).
//
// Hostile values (NaN, +-Infinity, -0, 1e308) are in the pools. Left out of the corpus because the
// TS hangs or exhausts memory (the Rust refuses, see the record): an arc sweep over 16384 quarter
// turns, a pattern raster over the pixel limit, a NaN pattern size, blip tiles under the step of the
// coordinate.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { CaptureWriter, Rng, Stream, importFrom, tsRoot } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : undefined; };
const outDir = opt('--out-dir') ?? join(here, '..', 'tests', 'captures');
const only = opt('--only');
const root = tsRoot(argv);
const dl = (p) => join(root, 'write/packages/docx-layout/dist', p);
const geometry = await import(pathToFileURL(dl('geometry.js')).href);
const pageFill = await import(pathToFileURL(dl('page-fill.js')).href);
const textfx = await import(pathToFileURL(dl('textfx.js')).href);
const { SfntFont } = await import(pathToFileURL(dl('typography/font/sfnt.js')).href);
const dml = await importFrom(root, 'shared/drawingml/dist/index.js');
const { patternRows, PRESET_PATTERNS } = await importFrom(root, 'shared/drawingml/dist/patterns.js');
const { newMask, presetOutline } = dml;

// ── helpers ─────────────────────────────────────────────────────────────────────────────────────

const canon = (v) => (v !== v ? NaN : v);
const f = (s, v) => s.f64(canon(v));
const dig = (fn) => { const s = new Stream(); fn(s); return `${s.items}:${s.finish().short}`; };
const U = undefined;

/** Source text of `function name(...) {...}` in a built file, run with `deps` in scope. */
function extractFn(file, name, deps = {}) {
  const src = readFileSync(file, 'utf8');
  const m = new RegExp(`(?:^|\\n)(?:export )?function ${name}\\(`).exec(src);
  if (!m) throw new Error(`${name} not found in ${file}`);
  const start = m.index + (src[m.index] === '\n' ? 1 : 0);
  let i = src.indexOf('{', src.indexOf(')', start));
  let depth = 0;
  for (; i < src.length; i++) {
    const c = src[i];
    if (c === '/' && src[i + 1] === '/') { while (src[i] !== '\n') i++; continue; }
    if (c === '/' && src[i + 1] === '*') { i = src.indexOf('*/', i) + 1; continue; }
    if (c === '"' || c === "'" || c === '`') {
      const q = c; i++;
      while (src[i] !== q) { if (src[i] === '\\') i++; i++; }
      continue;
    }
    if (c === '{') depth++;
    else if (c === '}') { depth--; if (depth === 0) { i++; break; } }
  }
  const text = src.slice(start, i).replace(/^export /, '');
  return new Function(...Object.keys(deps), `${text}\nreturn ${name};`)(...Object.values(deps));
}
const constText = (file, re) => { const m = re.exec(readFileSync(file, 'utf8')); if (!m) throw new Error(`${re} not in ${file}`); return m[0]; };

const GOOD = [0, 1, 2, 5, 10, 16.5, 50, 100, 150.25, 240, 720, 1440, 3000, 5760, 11906, 0.5, 0.25, 7, 33.333];
const BAD = [-0, -1, -5.5, 1e-7, 1e9, 1e15, NaN, Infinity, -Infinity, 1e308, -1e308];
const pick = (r, a) => a[r.below(a.length)];
const num = (r, bad = 12) => (r.below(100) < bad ? BAD[r.below(BAD.length)] : GOOD[r.below(GOOD.length)]);
const bool = (r) => r.below(2) === 1;

const COLORS = ['FF0000', '00FF00', '0000FF', 'FFFFFF', '000000', '123456', 'abcdef', 'ABCDEF', '', 'zz', '12', '-1f0a0b', ' 5a5a5a', '0x1234', 'ff', '+f+f+f', '7f7f7f', 'ffffff00', 'FF00', '8080GG'];
const VALID = ['000000', 'FF0000', '808080', '12AB34', 'FFFFFF', '00B050'];

// path commands as `{ op, ... }` and their digest
const cmdOut = (s, cmds) => {
  s.u32(cmds.length);
  for (const c of cmds) {
    s.tag(c.op);
    if (c.op === 'C') { f(s, c.x1); f(s, c.y1); f(s, c.x2); f(s, c.y2); f(s, c.x); f(s, c.y); }
    else if (c.op !== 'Z') { f(s, c.x); f(s, c.y); }
  }
};
const optNum = (s, v) => { if (v === undefined) s.tag('u'); else { s.tag('n'); f(s, v); } };

const files = {};
const cap = (name) => (files[name] ??= new CaptureWriter([`Parity capture of frontier G of uo-render (${name}): written by core/crates/uo-render/tools/capture-g.mjs, replayed by tests/g_${name}.rs.`]));
const want = (name) => !only || only === name;

// ── geom ────────────────────────────────────────────────────────────────────────────────────────

function genCmds(r, max = 7) {
  const out = [];
  const n = r.below(max);
  for (let i = 0; i < n; i++) {
    switch (r.below(4)) {
      case 0: out.push({ op: 'M', x: num(r), y: num(r) }); break;
      case 1: out.push({ op: 'L', x: num(r), y: num(r) }); break;
      case 2: out.push({ op: 'C', x1: num(r), y1: num(r), x2: num(r), y2: num(r), x: num(r), y: num(r) }); break;
      default: out.push({ op: 'Z' });
    }
  }
  return out;
}

const SWEEPS = [90, -90, 180, 270, 360, 45, 720, 1e-12, 89.9999999999, 91, -360, 0, NaN, 30.5, 1000];
function genCust(r) {
  const paths = [];
  const np = r.below(4);
  for (let p = 0; p < np; p++) {
    const w = pick(r, [0, 1, 100, NaN, -50, 2000, 1e9, 21600, 3.5]);
    const h = pick(r, [0, 1, 100, NaN, -50, 2000, 1e9, 21600, 3.5]);
    const fill = bool(r), stroke = bool(r);
    const cmds = [];
    const nc = r.below(9);
    for (let i = 0; i < nc; i++) {
      switch (r.below(8)) {
        case 0: case 6: cmds.push({ op: 'M', x: num(r), y: num(r) }); break;
        case 1: case 7: cmds.push({ op: 'L', x: num(r), y: num(r) }); break;
        case 2: { const k = r.below(5); const pts = []; for (let j = 0; j < k; j++) pts.push([num(r), num(r)]); cmds.push({ op: 'C', pts }); break; }
        case 3: { const k = r.below(4); const pts = []; for (let j = 0; j < k; j++) pts.push([num(r), num(r)]); cmds.push({ op: 'Q', pts }); break; }
        case 4: cmds.push({ op: 'A', wR: num(r), hR: num(r), st: num(r), sw: pick(r, SWEEPS) }); break;
        default: cmds.push({ op: 'Z' });
      }
    }
    paths.push({ w, h, fill, stroke, cmds });
  }
  return paths;
}

const PRESETS = ['triangle', 'rtTriangle', 'diamond', 'parallelogram', 'trapezoid', 'homePlate', 'chevron', 'hexagon', 'octagon', 'rightArrow', 'leftArrow', 'upArrow', 'downArrow', 'upDownArrow', 'leftRightArrow', 'ellipse', 'roundRect', 'round2SameRect', 'frame', 'bentConnector3', 'line', 'straightConnector1', 'rect', 'bentConnector2', 'curvedConnector3', 'star5', '', 'custom', 'Triangle'];
function genAdj(r) {
  switch (r.below(6)) {
    case 0: return undefined;
    case 1: return { adj: num(r) };
    case 2: return { adj1: num(r), adj2: num(r) };
    case 3: return { adj: num(r), adj1: num(r) };
    case 4: return { vf: num(r), adj: num(r) };
    default: return { adj1: num(r), adj2: num(r), adj: num(r) };
  }
}

function captureGeom() {
  const c = cap('geom');
  const put = (k, v) => c.set(k, 'v', v);
  // presetPathLocal + presetPath (flip inside the frame, rotation around the centre)
  PRESETS.forEach((prst, pi) => {
    const r = new Rng(1000 + pi);
    put(`preset/${prst}`, dig((s) => {
      for (let i = 0; i < 300; i++) {
        const w = num(r), h = num(r), adj = genAdj(r);
        cmdOut(s, geometry.presetPathLocal(prst, w, h, adj));
        const x = num(r), y = num(r), flipH = bool(r), flipV = bool(r);
        const rot = r.below(4) === 0 ? U : num(r);
        cmdOut(s, geometry.presetPath(prst, x, y, w, h, { adj, flipH, flipV, rot }));
      }
    }));
  });
  // custPaths
  {
    const r = new Rng(5000);
    put('cust', dig((s) => {
      for (let i = 0; i < 3000; i++) {
        const cust = genCust(r);
        const x = num(r), y = num(r), w = num(r), h = num(r), flipH = bool(r), flipV = bool(r);
        const rot = r.below(4) === 0 ? U : num(r);
        const parts = geometry.custPaths(cust, x, y, w, h, { flipH, flipV, rot });
        s.u32(parts.length);
        for (const p of parts) { cmdOut(s, p.cmds); s.bool(p.fill); s.bool(p.stroke); }
      }
    }));
  }
  // arcTo, through custPaths' own export? it is private: extracted
  {
    const arcTo = extractFn(join(root, 'write/packages/docx-layout/dist/geometry.js'), 'arcTo');
    const r = new Rng(6000);
    put('arc', dig((s) => {
      for (let i = 0; i < 3000; i++) {
        const out = [];
        const cur = [num(r), num(r)];
        const end = arcTo(out, cur, num(r), num(r), num(r), pick(r, SWEEPS));
        cmdOut(s, out); f(s, end[0]); f(s, end[1]);
      }
    }));
  }
  // rotatePath and the line path of both painters (private: extracted)
  const rotCanvas = extractFn(dl('canvas.js'), 'rotatePath');
  const rotPdf = extractFn(dl('pdf.js'), 'rotatePath');
  const lineCanvas = extractFn(dl('canvas.js'), 'shapePath');
  const linePdf = extractFn(dl('pdf.js'), 'linePath', { T: 20 });
  const genLine = (r) => (r.below(3) === 0 ? { x1: num(r), y1: num(r), x2: num(r), y2: num(r) } : U);
  for (const [name, rot] of [['rotate.canvas', rotCanvas], ['rotate.pdf', rotPdf]]) {
    const r = new Rng(7000);
    put(name, dig((s) => { for (let i = 0; i < 2000; i++) { const cmds = genCmds(r); cmdOut(s, rot(cmds, num(r), num(r), num(r))); } }));
  }
  {
    const r = new Rng(7100);
    put('line.canvas', dig((s) => { for (let i = 0; i < 2000; i++) cmdOut(s, lineCanvas('line', num(r), num(r), num(r), num(r), genLine(r), bool(r), bool(r))); }));
  }
  {
    const r = new Rng(7100);
    put('line.pdf', dig((s) => { for (let i = 0; i < 2000; i++) cmdOut(s, linePdf(num(r), num(r), num(r), num(r), genLine(r), bool(r), bool(r))); }));
  }
  // pathBounds, mapPath (a fixed affine), OPEN_PRESETS
  {
    const r = new Rng(7200);
    put('bounds', dig((s) => { for (let i = 0; i < 2000; i++) { const b = geometry.pathBounds(genCmds(r)); f(s, b.x0); f(s, b.y0); f(s, b.x1); f(s, b.y1); } }));
  }
  {
    const r = new Rng(7300);
    put('map', dig((s) => { for (let i = 0; i < 2000; i++) cmdOut(s, geometry.mapPath(genCmds(r), (x, y) => [x * 1.5 + y * 0.25, y - 3])); }));
  }
  put('open', dig((s) => { for (const n of [...PRESETS, 'bentConnector3 ', 'curvedConnector3', 'straightConnector3']) s.bool(geometry.OPEN_PRESETS.has(n)); }));
  // the shape parts of both painters (canvas.ts:439-451, pdf.ts:386-400, with the unit)
  const GEOMS = ['rect', 'line', 'straightConnector1', 'straightConnector3', 'custom', 'ellipse', 'triangle', 'roundRect', 'bentConnector3', '', 'straightConnect', 'star5'];
  for (const unit of ['canvas', 'pdf']) {
    const r = new Rng(8000);
    put(`parts.${unit}`, dig((s) => {
      for (let i = 0; i < 3000; i++) {
        const geom = pick(r, GEOMS);
        const adj = r.below(3) === 0 ? { adj: num(r), adj1: num(r) } : U;
        const cust = geom === 'custom' && r.below(4) > 0 ? genCust(r) : U;
        const flipH = bool(r), flipV = bool(r);
        const rot = r.below(3) === 0 ? U : num(r);
        const [x, y, w, h] = [num(r), num(r), num(r), num(r)];
        const line = genLine(r);
        const st = { geom, adj, cust, flipH, flipV, rot };
        const xf = { flipH: !!st.flipH, flipV: !!st.flipV, rot: st.rot ?? 0 };
        const isLine = st.geom === 'line' || st.geom.startsWith('straightConnector');
        let parts;
        if (isLine) {
          let cmds = unit === 'canvas' ? lineCanvas('line', x, y, w, h, line, !!st.flipH, !!st.flipV) : linePdf(x, y, w, h, line, !!st.flipH, !!st.flipV);
          if (st.rot) cmds = (unit === 'canvas' ? rotCanvas : rotPdf)(cmds, x + w / 2, y + h / 2, st.rot);
          parts = [{ cmds, fill: false, stroke: true, evenOdd: false }];
        } else if (st.geom === 'custom' && st.cust) {
          parts = geometry.custPaths(st.cust, x, y, w, h, xf).map((p) => ({ cmds: p.cmds, fill: p.fill, stroke: p.stroke, evenOdd: true }));
        } else {
          parts = [{ cmds: geometry.presetPath(st.geom, x, y, w, h, { ...(st.adj ? { adj: st.adj } : {}), ...xf }), fill: !geometry.OPEN_PRESETS.has(st.geom), stroke: true, evenOdd: false }];
        }
        s.u32(parts.length);
        for (const p of parts) { cmdOut(s, p.cmds); s.bool(p.fill); s.bool(p.stroke); s.bool(p.evenOdd); }
      }
    }));
  }
}

// ── fill ────────────────────────────────────────────────────────────────────────────────────────

const POS = [0, 1, 0.5, 0.25, 0.75, -0.5, 1.5, NaN, 0.3333, 0.999];
function genStops(r, colors = COLORS) {
  const k = r.below(10);
  const n = k === 0 ? 2 : k === 1 ? 2 : r.below(6);
  const stops = [];
  for (let i = 0; i < n; i++) {
    let pos = pick(r, POS);
    if (k === 0) pos = i === 0 ? 0 : 1;
    if (k === 1) pos = i === 0 ? -1 : 2;
    stops.push({ pos, color: pick(r, colors) });
  }
  return stops;
}
const FILLTO = [0, 0.25, 0.5, 0.75, 1, -0.1, NaN, 0.3];
const genFillTo = (r) => (r.below(2) === 0 ? U : { l: pick(r, FILLTO), t: pick(r, FILLTO), r: pick(r, FILLTO), b: pick(r, FILLTO) });
const PATHS = [U, 'circle', 'rect', 'shape', 'path', ''];

function captureFill() {
  const c = cap('fill');
  const put = (k, v) => c.set(k, 'v', v);
  {
    const r = new Rng(100);
    put('samples', dig((s) => {
      for (let i = 0; i < 1200; i++) {
        const stops = genStops(r);
        const n = pick(r, [1, 2, 3, 17, 64, 256, 512]);
        s.bytes(geometry.gradientSamples(stops, n));
      }
    }));
  }
  {
    const r = new Rng(200);
    put('geometry', dig((s) => {
      for (let i = 0; i < 3000; i++) {
        const g = { stops: [], angleDeg: num(r), scaled: bool(r) ? true : U, rotWithShape: pick(r, [U, true, false]), path: pick(r, PATHS), fillTo: genFillTo(r) };
        const box = { x: num(r), y: num(r), w: num(r), h: num(r), rot: r.below(3) === 0 ? U : num(r) };
        const pathBox = { x0: num(r), y0: num(r), x1: num(r), y1: num(r) };
        const o = geometry.gradientGeometry(g, box, pathBox);
        if (o.kind === 'axial') { s.tag('a'); f(s, o.x0); f(s, o.y0); f(s, o.x1); f(s, o.y1); } else { s.tag('r'); f(s, o.x0); f(s, o.y0); f(s, o.r); }
      }
    }));
  }
  {
    const names = [...Object.keys(PRESET_PATTERNS).sort(), 'nope', ''];
    const r = new Rng(300);
    put('pattern', dig((s) => {
      for (let i = 0; i < 400; i++) {
        const prst = pick(r, names);
        const fg = pick(r, COLORS), bg = pick(r, COLORS);
        const x = pick(r, [0, 100.5, -37, 1234.5, 5000.25, -10000]), y = pick(r, [0, 100.5, -37, 1234.5, 5000.25, -10000]);
        const w = pick(r, [0, -5, 1, 7.5, 90, 300]), h = pick(r, [0, -5, 1, 7.5, 90, 300]);
        const px = pick(r, [15, 0.75, 1, 20, 2.5]), k = pick(r, [1, 4, 2, 3]);
        const o = geometry.patternRaster({ prst, fg, bg }, x, y, w, h, px, k);
        if (!o) { s.tag('N'); continue; }
        s.tag('P'); s.bytes(o.data); s.u32(o.w); s.u32(o.h); f(s, o.x); f(s, o.y); f(s, o.cells.w); f(s, o.cells.h);
      }
    }));
  }
  {
    const r = new Rng(400);
    const NAT = [null, null, { width: 1, height: 1 }, { width: 16, height: 16 }, { width: 48, height: 32 }, { width: 100, height: 640 }, { width: 0, height: 10 }, { width: NaN, height: 10 }, { width: 640, height: 480 }];
    put('tiles', dig((s) => {
      for (let i = 0; i < 1500; i++) {
        const tile = r.below(4) === 0 ? U : { tx: pick(r, [0, 9525, -9525, 127000, 3175, 1e5]), ty: pick(r, [0, 9525, -9525, 127000, 3175, 1e5]), sx: pick(r, [100000, 50000, 200000, 12345, 0, -100000, NaN]), sy: pick(r, [100000, 50000, 200000, 12345, 0, -100000, NaN]), algn: pick(r, ['tl', 't', 'tr', 'l', 'ctr', 'r', 'bl', 'b', 'br', 'zz', '', U]) };
        const natural = pick(r, NAT);
        const fr = [0, 100, -50.5, 1440, 3000, 12000];
        const [x, y, w, h] = [pick(r, fr), pick(r, fr), pick(r, fr), pick(r, fr)];
        const emu = pick(r, [1 / 635, 1 / 12700, 1, 0.5]);
        const out = geometry.blipTiles({ tile }, x, y, w, h, emu, natural);
        s.u32(out.length);
        for (const t of out) { f(s, t.x); f(s, t.y); f(s, t.w); f(s, t.h); }
      }
    }));
  }
  {
    const r = new Rng(500);
    put('rect', dig((s) => {
      for (let i = 0; i < 300; i++) {
        const g = { stops: genStops(r), angleDeg: 0, path: pick(r, PATHS), fillTo: genFillTo(r) };
        s.bool(geometry.isRectGradient(g));
        const w = pick(r, [0, 1, 10, 33.5, 100, 150, -5, 4000]), h = pick(r, [0, 1, 10, 33.5, 100, 150, -5, 4000]);
        const ppu = pick(r, [2, 0.5, 1, 0.1, 15, 3.3]);
        const o = geometry.rectGradientRaster(g, w, h, ppu);
        s.u32(o.w); s.u32(o.h); s.bytes(o.data);
      }
      s.bool(geometry.isRectGradient(U));
    }));
  }
}

// ── page_fill ───────────────────────────────────────────────────────────────────────────────────

const ANGLES = [0, 45, 90, 135, 180, -45, -90, -135, 225, 270, 315, 30, 60, 10.5, 360, 720, -360, NaN, 1e9, 359.7, 44.6, 134.4, -44.6];
const FOCI = [0, 100, -100, 50, -50, 75, -75, 25, NaN, 74.9, 0.5, -0.4];
const UNIT = [0, 0.25, 0.5, 1, 0.9, -0.2, NaN, 0.1];
function genPageFill(r) {
  const type = pick(r, ['gradient', 'gradientRadial', 'pattern', 'frame', 'tile']);
  const f0 = { type, c1: pick(r, COLORS), c2: pick(r, COLORS), angle: pick(r, ANGLES), focus: pick(r, FOCI) };
  if (r.below(3) === 0) { const n = r.below(4); f0.stops = []; for (let i = 0; i < n; i++) f0.stops.push({ pos: pick(r, POS), color: pick(r, COLORS) }); }
  if (r.below(2) === 0) f0.focusPosition = [pick(r, UNIT), pick(r, UNIT)];
  if (r.below(2) === 0) f0.gradientCenter = true;
  if (r.below(2) === 0) { const n = r.below(10); f0.pattern = []; for (let i = 0; i < n; i++) f0.pattern.push(pick(r, [0, 255, 170, 85, 1, 128, -1, 256, 0.5, NaN, 1e10])); }
  if (r.below(2) === 0) f0.imagePx = { w: pick(r, [0, 1, 64, 100, 640, -1, NaN]), h: pick(r, [0, 1, 64, 100, 480, -1, NaN]), dpi: pick(r, [0, 96, 300, 72, -5, NaN, 150.5]) };
  return f0;
}

function capturePageFill() {
  const c = cap('page_fill');
  const put = (k, v) => c.set(k, 'v', v);
  {
    const r = new Rng(10);
    put('param', dig((s) => {
      for (let i = 0; i < 600; i++) {
        const fl = genPageFill(r);
        for (let j = 0; j < 21; j++) {
          const x = pick(r, UNIT), y = pick(r, UNIT);
          f(s, pageFill.fillParam(fl, x, y, false)); f(s, pageFill.fillParam(fl, x, y, true));
        }
      }
    }));
  }
  {
    const r = new Rng(20);
    put('tile_px', dig((s) => {
      for (let i = 0; i < 300; i++) {
        const fl = genPageFill(r);
        for (const pdf of [false, true]) { const t = pageFill.gradientTilePx(fl, pdf); f(s, t.w); f(s, t.h); }
      }
    }));
  }
  {
    const r = new Rng(30);
    const DIMS = [[1, 1], [2, 3], [17, 9], [33, 1], [1, 40], [64, 48], [0, 5], [5, 0], [100, 80]];
    put('tile_rgba', dig((s) => {
      for (let i = 0; i < 160; i++) {
        const fl = genPageFill(r);
        const [w, h] = pick(r, DIMS);
        s.bytes(pageFill.gradientTileRgba(fl, w, h, false));
        s.bytes(pageFill.gradientTileRgba(fl, w, h, true));
      }
    }));
  }
  {
    const r = new Rng(40);
    put('pattern_rgba', dig((s) => { for (let i = 0; i < 400; i++) s.bytes(pageFill.patternRgba(genPageFill(r))); }));
  }
  {
    const r = new Rng(50);
    const DEV = [2.4, 15, 10, 20, 0, NaN, 7.5];
    put('tile_tw', dig((s) => {
      for (let i = 0; i < 600; i++) {
        const fl = genPageFill(r);
        const o = pageFill.pageFillTileTw(fl, pick(r, DEV), pick(r, DEV), bool(r));
        if (!o) s.tag('N'); else { s.tag('T'); f(s, o.w); f(s, o.h); }
      }
    }));
  }
}

// ── text_fx ─────────────────────────────────────────────────────────────────────────────────────

const FONT_FILES = ['liberationserif-regular', 'carlito-regular', 'liberationmono-bold', 'caladea-italic'];
const TEXT = [...'Hello, Wörld! fi ffl ÀÉÇ 123 gjpqy'].map((ch) => ch.codePointAt(0));
const CPS_BAD = [0xffff, 0x4e00, 0x10ffff, 0x20, 0];

function genRun(r, fonts, maxGlyphs = 8) {
  const font = fonts[r.below(fonts.length)];
  const n = r.below(maxGlyphs + 1);
  const glyphs = [];
  let x = pick(r, [0, 1440, -200, 5000.5]);
  for (let i = 0; i < n; i++) {
    const cp = r.below(12) === 0 ? pick(r, CPS_BAD) : pick(r, TEXT);
    const g = { cp, x };
    if (r.below(5) === 0) g.gid = pick(r, [0, 1, 36, 70, 499, 70000, 3]);
    glyphs.push(g);
    x += r.below(700);
  }
  return {
    face: { font }, sizePt: pick(r, [0, 9, 11, 14.5, 36, 100, 250, 0.5]), y: pick(r, [0, 240, 1440.5, -100]), glyphs,
    ...(r.below(2) === 0 ? { scaleX: pick(r, [1, 0.5, 2, 0, NaN, -1]) } : {}), ...(bool(r) ? { synthItalic: true } : {}),
  };
}
const ALGN = ['b', 't', 'tl', 'ctr', 'zz', 'br'];
function genFx(r) {
  const fx = {};
  if (r.below(8) !== 0) {
    if (bool(r)) fx.shadow = { blurPt: pick(r, [0, 2, 5.5, 10]), distPt: pick(r, [0, 3, -4, 8]), dirDeg: pick(r, [0, 45, 90, 135, 180, 270, 30.5]), sx: pick(r, [1, 0.5, 1.2, -1, 0.8]), sy: pick(r, [1, 0.5, 1.2, -1, 0.8]), kxDeg: pick(r, [0, 10, -20]), kyDeg: pick(r, [0, 10, -20]), algn: pick(r, ALGN), hex: pick(r, VALID), alpha: pick(r, [1, 0.5, 0.25, 0, 0.9]) };
    if (bool(r)) fx.glow = { radPt: pick(r, [0, 1.5, 4, 8]), hex: pick(r, VALID), alpha: pick(r, [1, 0.5, 0.25, 0, 0.9]) };
    if (bool(r)) fx.reflection = { blurPt: pick(r, [0, 1, 3]), stA: pick(r, [0.5, 1, 0.3]), stPos: pick(r, [0, 0.1]), endA: pick(r, [0, 0.1]), endPos: pick(r, [0.5, 0.9, 1]), distPt: pick(r, [0, 2, -1]), dirDeg: pick(r, [90, 0, 45]), algn: pick(r, ALGN) };
  }
  if (r.below(3) === 0) fx.outline = { widthPt: pick(r, [0.5, 1, 2.25, 0, 6, 12]), fill: { kind: 'none' }, cap: 'flat', join: 'miter' };
  if (r.below(4) === 0) fx.fill = { kind: 'none' };
  return fx;
}

function captureTextFx(fontBytes) {
  const c = cap('text_fx');
  const put = (k, v) => c.set(k, 'v', v);
  const fonts = fontBytes.map((b) => new SfntFont(b));
  const dilate = extractFn(dl('textfx.js'), 'dilate', { newMask });
  const subOut = (s, subs) => {
    s.u32(subs.length);
    for (const sub of subs) {
      s.u32(sub.length);
      for (const g of sub) {
        s.tag(g.c);
        if (g.c === 'Q') { f(s, g.x1); f(s, g.y1); f(s, g.x); f(s, g.y); } else { f(s, g.x); f(s, g.y); }
      }
    }
  };
  {
    const r = new Rng(60);
    put('margin', dig((s) => { for (let i = 0; i < 1500; i++) { const m = textfx.fxMarginPt(genFx(r), pick(r, [0, 12, 30.5, NaN, 100, -3])); f(s, m.l); f(s, m.t); f(s, m.r); f(s, m.b); } }));
  }
  {
    const r = new Rng(70);
    put('box', dig((s) => {
      for (let i = 0; i < 1500; i++) {
        const run = genRun(r, fonts);
        if (!run.glyphs.length) { s.tag('E'); continue; }
        const b = textfx.runBox(run);
        s.tag('B'); f(s, b.x); f(s, b.y); f(s, b.w); f(s, b.h);
      }
    }));
  }
  {
    const r = new Rng(80);
    put('outline', dig((s) => { for (let i = 0; i < 600; i++) subOut(s, textfx.runOutlineSegs(genRun(r, fonts))); }));
  }
  {
    const r = new Rng(90);
    const gids = [{ kind: 'grad', stops: [{ pos: 0, hex: 'FF0000', alpha: 1 }, { pos: 1, hex: '0000FF', alpha: 0.5 }], angleDeg: 90, scaled: true }, { kind: 'grad', stops: [], angleDeg: NaN, scaled: false, path: 'circle', fillTo: { l: 0.5, t: 0.5, r: 0.5, b: 0.5 } }];
    put('gradof', dig((s) => {
      for (const g of gids) {
        const o = textfx.gradOf(g);
        s.u32(o.stops.length);
        for (const st of o.stops) { f(s, st.pos); s.str(st.color); }
        f(s, o.angleDeg); s.bool(!!o.scaled); s.str(o.path ?? '~'); s.bool(o.rotWithShape === undefined);
        s.bool(!!o.fillTo); if (o.fillTo) { f(s, o.fillTo.l); f(s, o.fillTo.t); f(s, o.fillTo.r); f(s, o.fillTo.b); }
      }
      void r;
    }));
  }
  {
    const r = new Rng(100);
    put('dilate', dig((s) => {
      for (let i = 0; i < 300; i++) {
        const w = r.below(40), h = r.below(30);
        const m = newMask(w, h);
        for (let k = 0; k < m.a.length; k++) m.a[k] = r.below(5) === 0 ? pick(r, [1, 0.5, 0.25, 2, NaN, 0.999]) : 0;
        const o = dilate(m, pick(r, [0.5, 1, 1.5, 2.2, 3, 5.5, 0.4, -1, 0, NaN, 12]));
        s.u32(o.w); s.u32(o.h);
        const fl = new Float32Array(1), ui = new Uint32Array(fl.buffer), u = new Uint32Array(o.a.length);
        for (let k = 0; k < u.length; k++) { fl[0] = o.a[k]; u[k] = fl[0] !== fl[0] ? 0x7fc00000 : ui[0]; }
        s.bytes(Buffer.from(u.buffer));
      }
    }));
  }
  {
    const names = ['dot', 'sysDot', 'sysDash', 'dash', 'dashDot', 'lgDash', 'lgDashDot', 'lgDashDotDot', 'sysDashDot', 'sysDashDotDot', 'solid', '', U, 'DASH', 'dash '];
    put('dash', dig((s) => { for (const n of names) { const a = textfx.dashPattern(n); s.u32(a.length); for (const v of a) f(s, v); } }));
  }
  {
    const r = new Rng(110);
    put('layers', dig((s) => {
      for (let i = 0; i < 260; i++) {
        const run = genRun(r, fonts, 6);
        run.sizePt = pick(r, [8, 11, 14.5, 24, 36]);
        // a NaN size gives the TS a degenerate layer of NaN size (the Rust returns none): outside the corpus
        if (run.scaleX !== run.scaleX) run.scaleX = 1;
        const fx = genFx(r);
        const ppt = pick(r, [0.5, 1, 2.78, 4]);
        const hex = pick(r, VALID);
        if (!run.glyphs.length) { s.tag('E'); continue; }
        const o = textfx.fxLayers(run, fx, ppt, hex);
        if (!o) { s.tag('N'); continue; }
        s.tag('L'); s.u32(o.w); s.u32(o.h); f(s, o.x0); f(s, o.y0); s.bytes(o.data);
      }
    }));
  }
}

// ── picture ─────────────────────────────────────────────────────────────────────────────────────

function genLook(r) {
  const look = {};
  if (r.below(2) === 0) look.line = { widthPt: num(r) };
  if (r.below(2) === 0) look.glow = { radPt: num(r) };
  if (r.below(3) > 0) {
    const sh = { kind: pick(r, ['outer', 'outer', 'inner']), blurPt: num(r), distPt: num(r), dirDeg: num(r) };
    if (bool(r)) sh.sx = num(r);
    if (bool(r)) sh.sy = bool(r) ? -Math.abs(num(r, 0)) : num(r);
    if (bool(r)) sh.kxDeg = num(r);
    if (bool(r)) sh.kyDeg = num(r);
    look.shadow = sh;
  }
  if (r.below(2) === 0) look.reflection = { blurPt: num(r), distPt: num(r), dirDeg: num(r) };
  return look;
}
const SHAPES = ['rect', 'ellipse', 'star5', 'heart', 'roundRect', 'rightArrow', 'cloud', 'nope', '', U, 'wedgeRectCallout', 'arc', 'pie', 'chord', 'blockArc', 'triangle', 'flowChartDecision'];

function capturePicture() {
  const c = cap('picture');
  const put = (k, v) => c.set(k, 'v', v);
  const cp = dl('canvas-picture.js'), pp = dl('pdf-picture.js');
  const boundsCanvas = extractFn(cp, 'bounds');
  const boundsPdf = extractFn(pp, 'bounds');
  const subsCanvas = extractFn(cp, 'shapeSubs', { presetOutline });
  const subsPdf = extractFn(pp, 'shapeSubs', { presetOutline });
  const rgbSrc = (file) => constText(file, /const rgb = [^\n]*;/);
  const toCanvas = new Function('canvas', 'ctx2d', `${rgbSrc(cp)}\n${readFnText(cp, 'maskToCanvas')}\nreturn maskToCanvas;`);
  const maskToCanvas = toCanvas((w, h) => ({ w, h }), (cv) => ({ createImageData: (w, h) => ({ data: new Uint8ClampedArray(w * h * 4) }), putImageData: (img) => { cv.img = img; } }));
  const colored = new Function(`${rgbSrc(pp)}\n${readFnText(pp, 'colored')}\nreturn colored;`)();
  const boundsOut = (s, b) => { f(s, b.x0); f(s, b.y0); f(s, b.x1); f(s, b.y1); };
  {
    const r = new Rng(120);
    put('bounds.canvas', dig((s) => { for (let i = 0; i < 3000; i++) { const look = r.below(10) === 0 ? U : genLook(r); boundsOut(s, boundsCanvas(look, num(r), num(r))); } }));
  }
  {
    const r = new Rng(120);
    put('bounds.pdf', dig((s) => { for (let i = 0; i < 3000; i++) { const look = r.below(10) === 0 ? {} : genLook(r); boundsOut(s, boundsPdf(look, num(r), num(r))); } }));
  }
  {
    const r = new Rng(130);
    put('subs', dig((s) => {
      for (let i = 0; i < 800; i++) {
        const prst = pick(r, SHAPES);
        const adj = r.below(3) === 0 ? { adj1: pick(r, [0, 25000, 50000, 100000, NaN]), adj2: pick(r, [0, 25000, 50000, 16667]) } : U;
        const look = r.below(10) === 0 ? U : { geom: { prst, ...(adj ? { adj } : {}) } };
        const w = pick(r, [10, 100, 150.5, 0.5, 0, 720]), h = pick(r, [10, 100, 150.5, 0.5, 0, 720]);
        const flipH = bool(r), flipV = bool(r);
        const a = subsCanvas(look, w, h, flipH, flipV), b = subsPdf(look, w, h, flipH, flipV);
        const da = dig((t) => subsOut(t, a)), db = dig((t) => subsOut(t, b));
        if (da !== db) throw new Error('the two shapeSubs copies differ');
        subsOut(s, a);
      }
    }));
  }
  const HOSTILE = [0, -0, 0.25, 0.5, 1, 2, -1, NaN, 0.5, 0.4999999];
  const genMask = (r) => { const w = 1 + r.below(6), h = 1 + r.below(5); const m = newMask(w, h); for (let i = 0; i < m.a.length; i++) m.a[i] = pick(r, HOSTILE); return m; };
  for (const kind of ['canvas', 'pdf']) {
    const r = new Rng(140);
    put(`colored.${kind}`, dig((s) => {
      for (let i = 0; i < 600; i++) {
        const m = genMask(r), hex = pick(r, COLORS), alpha = pick(r, [1, 0.5, 0, NaN, 2, -1, 0.3333]);
        if (kind === 'canvas') { const cv = maskToCanvas(m, hex, alpha); s.bytes(Buffer.from(cv.img.data.buffer)); } else s.bytes(Buffer.from(colored(m, hex, alpha).buffer));
      }
    }));
  }
  {
    const dc = new Function(`${constText(cp, /const DASH = \{[^}]*\};/)}\nreturn DASH;`)();
    const dp = new Function(`${constText(pp, /const DASH = \{[^}]*\};/)}\nreturn DASH;`)();
    if (JSON.stringify(dc) !== JSON.stringify(dp)) throw new Error('the two DASH tables differ');
    const names = ['sysDot', 'sysDash', 'dash', 'dashDot', 'lgDash', 'lgDashDot', 'lgDashDotDot', 'sysDashDot', 'sysDashDotDot', 'dot', 'solid', '', 'DASH'];
    put('dash', dig((s) => { for (const n of names) { const a = dc[n]; if (!a) s.tag('N'); else { s.tag('D'); s.u32(a.length); for (const v of a) f(s, v); } } }));
  }
}
function subsOut(s, subs) {
  s.u32(subs.length);
  for (const sub of subs) {
    s.u32(sub.length);
    for (const g of sub) {
      s.tag(g.c);
      if (g.c === 'C') { f(s, g.x1); f(s, g.y1); f(s, g.x2); f(s, g.y2); f(s, g.x); f(s, g.y); }
      else if (g.c === 'Q') { f(s, g.x1); f(s, g.y1); f(s, g.x); f(s, g.y); }
      else if (g.c === 'E') { f(s, g.cx); f(s, g.cy); f(s, g.rx); f(s, g.ry); f(s, g.a0); f(s, g.a1); s.bool(!!g.ccw); }
      else if (g.c !== 'Z') { f(s, g.x); f(s, g.y); }
    }
  }
}
function readFnText(file, name) {
  // source text of the function (for `new Function` with several declarations)
  const src = readFileSync(file, 'utf8');
  const m = new RegExp(`(?:^|\\n)function ${name}\\(`).exec(src);
  const start = m.index + (src[m.index] === '\n' ? 1 : 0);
  let i = src.indexOf('{', src.indexOf(')', start)), depth = 0;
  for (; i < src.length; i++) { if (src[i] === '{') depth++; else if (src[i] === '}') { depth--; if (depth === 0) { i++; break; } } }
  return src.slice(start, i);
}

// ── main ────────────────────────────────────────────────────────────────────────────────────────

const fontBytes = FONT_FILES.map((n) => new Uint8Array(readFileSync(join(root, 'shared/fonts/files', `${n}.ttf`))));
const t0 = Date.now();
if (want('geom')) captureGeom();
if (want('fill')) captureFill();
if (want('page_fill')) capturePageFill();
if (want('text_fx')) captureTextFx(fontBytes);
if (want('picture')) capturePicture();
for (const [name, w] of Object.entries(files)) {
  w.write(join(outDir, `g_${name}.capture`));
  console.log(`g_${name}.capture: ${w.records.size} records`);
}
console.log(`${Date.now() - t0} ms`);
