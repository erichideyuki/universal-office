// Parity capture of the VIEWPORT op stream (frontier C4, record core/docs/modules/uo-xlsx.md "C4 — as built"): the frozen TS
// `paint()` of the real `<universal-sheet>` element on the fake DOM (T10a) over a RECORDING 2D context, per sheet at several
// scroll / zoom / selection / frozen-pane states. The recorded canvas calls are canonicalised (state assignments that do
// not change the state are dropped with the save/restore stack, paths are flushed at fill/stroke/clip, floats are IEEE bits,
// colours lower case, cell fonts reduced to `weight ppem family`) and compared by digest with the Rust op stream after
// `GlyphRun` expansion (`tests/viewport_parity.rs`, `tests/support/vp_canon.rs` implements the same canonicalisation).
//
//   node core/crates/uo-xlsx/js/capture-viewport.mjs                 # -> tests/captures/sheet-viewport.capture
//   node core/crates/uo-xlsx/js/capture-viewport.mjs --lines <dir>   # also every canonical line (gz per workbook) for diagnosis
//   node core/crates/uo-xlsx/js/capture-viewport.mjs --big           # local: x1-big (400) + paint-big + cf1 -> core/target/sheet-cap/viewport-big.capture
//
// Facets per workbook key: `v<i>@<k>` digest of the canonical lines of sheet i (view index) at state k, `n<i>@<k>` their count.
//
// C4b: the drawing phases (`paintDrawings` per quadrant, `paintChartsheet`) are recorded apart, each call on a fresh RECORDING
// context of the Slides harness (`uo-pptx/js/lib/record-ctx.mjs`, the deterministic measure stub, images as hashed fake decoded
// images), and compared by `js/compare-drawings.mjs` with the Rust list replayed by `paint-slides.ts`: `d<i>@<k>` digest of the
// draw-form lines of the frame's drawing calls (`#call` before each), `dc<i>@<k>` their count; `stacks` the shape font stacks the
// frozen element gave (`family|bold|italic` -> stack, JSON). Chart sheets are captured too (main stream: transform and clear).
process.env.TZ = 'UTC';
const { readFileSync, mkdirSync, writeFileSync, existsSync } = await import('node:fs');
const { join } = await import('node:path');
const { createHash } = await import('node:crypto');
const { gzipSync } = await import('node:zlib');
const { pathToFileURL } = await import('node:url');
const { CaptureWriter } = await import('../../uo-testkit/js/testkit.mjs');
const C = await import('./lib/common.mjs');
const { GROUPS, groupKey, workbookFile } = await import('./lib/gen-workbook.mjs');
const { PAINT_GROUPS, paintKey, buildPaintWorkbook } = await import('./lib/gen-paint.mjs');
const { CF_GROUPS, cfKey, cfFile } = await import('./lib/gen-cf.mjs');
const { DRAW_GROUPS, drawKey, buildDrawingWorkbook } = await import('./lib/gen-drawings.mjs');
const { buildHarness } = await import('./harness/build-harness.mjs');
const { createRecorder } = await import('../../uo-pptx/js/lib/record-ctx.mjs');
const { form } = await import('../../uo-pptx/js/lib/opstream.mjs');

const FIXED = Date.UTC(2025, 0, 15, 12, 0, 0);
const RealDate = Date;
globalThis.Date = class FixedDate extends RealDate {
  constructor(...x) { if (x.length === 0) super(FIXED); else super(...x); }
  static now() { return FIXED; }
};

const a = C.args();
const root = C.tsRoot(a);
const BIG = a.flag('--big');
const LINES_DIR = a.opt('--lines', undefined);
const outPath = a.opt('--out', BIG ? join(C.bigDir, 'viewport-big.capture') : join(C.capturesDir, 'sheet-viewport.capture'));

const h = buildHarness(root, join(C.coreDir, 'target', 'sheet-cap', 'harness'));
const domMod = await import(pathToFileURL(h.dom).href);
const dom = domMod.installFakeDom();
dom.customElements.define('u-ripple', class extends globalThis.HTMLElement {});
const M = await import(pathToFileURL(h.sheet).href);
globalThis.DOMMatrix = class { constructor(m) { [this.a, this.b, this.c, this.d, this.e, this.f] = m; } };

const sha = (b) => createHash('sha256').update(b).digest('hex');
const dig = (s) => { const b = Buffer.from(s, 'utf8'); return `${b.length}:${sha(b).slice(0, 32)}`; };
// -0 and +0 paint the same: the sign of zero is normalised (`Math.round(-0.3)` is -0 in the TS)
const fh = (x) => (Number.isNaN(x) ? 'nan' : C.f64hex(x === 0 ? 0 : x));
const lc = (s) => String(s).toLowerCase();

// ------------------------------------------------------------ recording canvases (tiles, rasters, the measurer)
let stubOn = false;
function stubCanvas() {
  const rec = { bg: undefined, fg: undefined, rows: Array(8).fill(0) };
  const base = {
    fillStyle: '#000',
    fillRect(x, y, ww, hh) {
      if (ww === 8 && hh === 8) { rec.bg = lc(this.fillStyle); return; }
      rec.fg = lc(this.fillStyle);
      rec.rows[y] |= 1 << (7 - x);
    },
    measureText: (t) => ({ width: t.length * 6, actualBoundingBoxAscent: 8, actualBoundingBoxDescent: 2, fontBoundingBoxAscent: 9, fontBoundingBoxDescent: 3 }),
    createImageData(ww, hh) { return { width: ww, height: hh, data: new Uint8ClampedArray(ww * hh * 4) }; },
    putImageData(img) { rec.img = sha(Buffer.from(img.data.buffer, img.data.byteOffset, img.data.byteLength)).slice(0, 16); },
  };
  const ctx = new Proxy(base, { get(t, k) { return k in t ? t[k] : () => undefined; }, set(t, k, v) { t[k] = v; return true; } });
  return { width: 0, height: 0, rec, getContext: () => ctx };
}
const origCreate = dom.document.createElement.bind(dom.document);
dom.document.createElement = (tag) => (stubOn && tag === 'canvas' ? stubCanvas() : origCreate(tag));

const hexRgb = (c) => { const m = /^#?([0-9a-f]{6})/i.exec(c); if (!m) return [0, 0, 0]; const n = parseInt(m[1], 16); return [(n >> 16) & 255, (n >> 8) & 255, n & 255]; };
/** The RGBA of a pattern tile as `patternTile` drew it (bg everywhere, fg where the mask has a bit). */
function tileSha(rec) {
  const bg = hexRgb(rec.bg ?? '#ffffff'), fg = hexRgb(rec.fg ?? '#000000');
  const px = Buffer.alloc(8 * 8 * 4);
  for (let r = 0; r < 8; r++) for (let c = 0; c < 8; c++) {
    const on = (rec.rows[r] >> (7 - c)) & 1;
    const rgb = on ? fg : bg;
    const o = (r * 8 + c) * 4;
    px[o] = rgb[0]; px[o + 1] = rgb[1]; px[o + 2] = rgb[2]; px[o + 3] = 255;
  }
  return sha(px).slice(0, 16);
}

// ------------------------------------------------------------ the recording 2D context of the element
class Path2DStub {
  constructor() { this.ops = []; }
  moveTo(x, y) { this.ops.push(`M ${fh(x)} ${fh(y)}`); }
  lineTo(x, y) { this.ops.push(`L ${fh(x)} ${fh(y)}`); }
  arc(x, y, r, a0, a1, ccw) { this.ops.push(`A ${fh(x)} ${fh(y)} ${fh(r)} ${fh(a0)} ${fh(a1)} ${ccw ? 1 : 0}`); }
  rect(x, y, ww, hh) { this.ops.push(`R ${fh(x)} ${fh(y)} ${fh(ww)} ${fh(hh)}`); }
  roundRect(x, y, ww, hh, r) { this.ops.push(`RR ${fh(x)} ${fh(y)} ${fh(ww)} ${fh(hh)} ${fh(r)}`); }
  closePath() { this.ops.push('Z'); }
}
globalThis.Path2D = Path2DStub;

const SET_KEYS = new Set(['fillStyle', 'strokeStyle', 'lineWidth', 'font', 'textAlign', 'textBaseline', 'lineJoin', 'lineCap', 'miterLimit', 'globalAlpha']);

function styleText(s) {
  if (typeof s === 'string') return lc(s);
  if (s?.kind === 'lin') return `L(${s.p.map(fh).join(',')};${s.stops.map(([p, c]) => `${fh(p)}=${lc(c)}`).join(',')})`;
  if (s?.kind === 'pat') return `PAT(${tileSha(s.tile.rec)}/${s.tr ? s.tr.map(fh).join(',') : '-'})`;
  return '?';
}

function fontText(f) {
  const m = /^(italic )?(400|700) (\d+)px (.*)$/.exec(f);
  if (!m) return f;
  const fams = m[4].split(',').map((x) => x.trim().replace(/^['"]|['"]$/g, ''));
  // C4b: the alternate name of the font (`altName`) is the stack's second family (`cssStack(name, [alt, ...SHEET_FALLBACKS])`;
  // without it the second is the fallback `Arial`)
  const fam = fams[1] && fams[1] !== 'Arial' && fams[1] !== 'Liberation Sans' ? `${fams[0]}|${fams[1]}` : fams[0];
  return `${m[1] ?? ''}${m[2]} ${m[3]} ${fam}`;
}

/** Records the canvas calls as raw events; `canon()` canonicalises them. */
function recorder() {
  const ev = [];
  let cur = [];
  // the current transform (C4b: `drawCellText` reads `getTransform().a` to unmirror the text of a right-to-left camera render)
  let tm = [1, 0, 0, 1, 0, 0];
  const tms = [];
  const mul = (n) => { const [a, b, c, d, e, f] = tm; tm = [a * n[0] + c * n[1], b * n[0] + d * n[1], a * n[2] + c * n[3], b * n[2] + d * n[3], a * n[4] + c * n[5] + e, b * n[4] + d * n[5] + f]; };
  const ctx = {
    canvas: { width: 0, height: 0 },
    setTransform(...m) { ev.push(`setTransform ${m.map(fh).join(' ')}`); tm = m.slice(0, 6); },
    getTransform() { const [a, b, c, d, e, f] = tm; return { a, b, c, d, e, f }; },
    save() { ev.push('save'); tms.push(tm); }, restore() { ev.push('restore'); tm = tms.pop() ?? tm; },
    translate(x, y) { ev.push(`translate ${fh(x)} ${fh(y)}`); mul([1, 0, 0, 1, x, y]); },
    scale(x, y) { ev.push(`scale ${fh(x)} ${fh(y)}`); mul([x, 0, 0, y, 0, 0]); },
    rotate(r) { ev.push(`rotate ${fh(r)}`); mul([Math.cos(r), Math.sin(r), -Math.sin(r), Math.cos(r), 0, 0]); },
    clearRect(x, y, ww, hh) { ev.push(`clearRect ${[x, y, ww, hh].map(fh).join(' ')}`); },
    fillRect(x, y, ww, hh) { ev.push(`fillRect ${[x, y, ww, hh].map(fh).join(' ')}`); },
    strokeRect(x, y, ww, hh) { ev.push(`strokeRect ${[x, y, ww, hh].map(fh).join(' ')}`); },
    fillText(t, x, y) { ev.push(`fillText ${JSON.stringify(t)} ${fh(x)} ${fh(y)}`); },
    beginPath() { cur = []; ev.push('beginPath'); },
    moveTo(x, y) { ev.push(`M ${fh(x)} ${fh(y)}`); },
    lineTo(x, y) { ev.push(`L ${fh(x)} ${fh(y)}`); },
    arc(x, y, r, a0, a1, ccw) { ev.push(`A ${fh(x)} ${fh(y)} ${fh(r)} ${fh(a0)} ${fh(a1)} ${ccw ? 1 : 0}`); },
    rect(x, y, ww, hh) { ev.push(`R ${fh(x)} ${fh(y)} ${fh(ww)} ${fh(hh)}`); },
    roundRect(x, y, ww, hh, r) { ev.push(`RR ${fh(x)} ${fh(y)} ${fh(ww)} ${fh(hh)} ${fh(r)}`); },
    closePath() { ev.push('Z'); },
    fill(p, rule) { if (typeof p === 'string') { rule = p; p = undefined; } ev.push(p ? `fillP ${rule ?? 'nonzero'} ${p.ops.join(',')}` : `fill ${rule ?? 'nonzero'}`); },
    stroke(p) { ev.push(p ? `strokeP ${p.ops.join(',')}` : 'stroke'); },
    clip(p, rule) { if (typeof p === 'string') { rule = p; p = undefined; } ev.push(p ? `clipP ${rule ?? 'nonzero'} ${p.ops.join(',')}` : `clip ${rule ?? 'nonzero'}`); },
    setLineDash(d) { ev.push(`S lineDash [${d.map(fh).join(',')}]`); },
    createLinearGradient(x0, y0, x1, y1) { return { kind: 'lin', p: [x0, y0, x1, y1], stops: [], addColorStop(p, c) { this.stops.push([p, c]); } }; },
    createPattern(tile) { return { kind: 'pat', tile, tr: undefined, setTransform(m) { this.tr = [m.e, m.f]; } }; },
    drawImage(off, x, y, ww, hh) { ev.push(`drawImage ${off.rec?.img ?? '?'} ${[x, y, ww, hh].map(fh).join(' ')}`); },
    measureText: (t) => ({ width: t.length * 6, actualBoundingBoxAscent: 8, actualBoundingBoxDescent: 2, fontBoundingBoxAscent: 9, fontBoundingBoxDescent: 3 }),
  };
  const store = {};
  const proxy = new Proxy(ctx, {
    get(t, k) { return k in t ? t[k] : store[k]; },
    set(t, k, v) {
      if (SET_KEYS.has(k)) {
        const text = k === 'fillStyle' || k === 'strokeStyle' ? styleText(v) : k === 'font' ? fontText(v) : typeof v === 'number' ? fh(v) : String(v);
        ev.push(`S ${k} ${text}`);
      }
      store[k] = v;
      return true;
    },
  });
  void cur;
  return { ctx: proxy, ev };
}

/** Canonical lines: redundant assignments dropped (state stack), paths flushed at fill/stroke/clip. */
export function canon(ev) {
  const out = [];
  let state = {};
  const stack = [];
  let path = [];
  for (const e of ev) {
    if (e === 'save') { stack.push({ ...state }); out.push(e); continue; }
    if (e === 'restore') { state = stack.pop() ?? {}; out.push(e); continue; }
    if (e === 'beginPath') { path = []; continue; }
    if (/^(M|L|A|R|RR|Z)( |$)/.test(e)) { path.push(e); continue; }
    if (e.startsWith('S ')) {
      const i = e.indexOf(' ', 2);
      const k = e.slice(2, i), v = e.slice(i + 1);
      if (state[k] === v) continue;
      state[k] = v;
      out.push(e);
      continue;
    }
    if (e.startsWith('fill ') || e.startsWith('clip ')) { out.push(`${e.split(' ')[0]} ${e.split(' ')[1]} ${path.join(',')}`); continue; }
    if (e === 'stroke') { out.push(`stroke ${path.join(',')}`); continue; }
    if (e.startsWith('fillP ')) { const [, rule, ops] = /^fillP (\S+) ?(.*)$/.exec(e); out.push(`fill ${rule} ${ops}`); continue; }
    if (e.startsWith('clipP ')) { const [, rule, ops] = /^clipP (\S+) ?(.*)$/.exec(e); out.push(`clip ${rule} ${ops}`); continue; }
    if (e.startsWith('strokeP')) { out.push(`stroke ${e.slice(8)}`); continue; }
    out.push(e);
  }
  return out;
}

// ------------------------------------------------------------ the element
/** Natural size of a PNG, JPEG or GIF (`ts-paint.mjs` `imageSize`); null when unknown. */
function imageSize(b) {
  if (b.length > 24 && b[0] === 0x89 && b[1] === 0x50) return { w: (b[16] << 24 | b[17] << 16 | b[18] << 8 | b[19]) >>> 0, h: (b[20] << 24 | b[21] << 16 | b[22] << 8 | b[23]) >>> 0 };
  if (b.length > 10 && b[0] === 0x47 && b[1] === 0x49) return { w: b[6] | b[7] << 8, h: b[8] | b[9] << 8 };
  if (b.length > 4 && b[0] === 0xff && b[1] === 0xd8) {
    let i = 2;
    while (i + 9 < b.length) {
      if (b[i] !== 0xff) { i++; continue; }
      const mk = b[i + 1];
      if (mk >= 0xc0 && mk <= 0xcf && mk !== 0xc4 && mk !== 0xc8 && mk !== 0xcc) return { h: b[i + 5] << 8 | b[i + 6], w: b[i + 7] << 8 | b[i + 8] };
      i += 2 + (b[i + 2] << 8 | b[i + 3]);
    }
  }
  return null;
}
const sha1 = (b) => createHash('sha1').update(b).digest('hex').slice(0, 12);

/** A drawing call (`paintDrawings`, `paintChartsheet`) recorded on a fresh Slides recording context: its draw-form lines. */
function recordCall(fn) {
  const rec = createRecorder({ width: W, height: H });
  rec.install();
  try { fn(rec.ctx); } catch (e) { rec.root.lines.push(`#throw ${C.oneLine(e?.message ?? e)}`); } finally { rec.uninstall(); }
  // `@cvK` lines (offscreen canvases: pattern tiles, layers) are left out: the module-level canvas caches of the frozen painter
  // make them depend on what was painted before; each canvas still enters the stream by its content hash where it is drawn
  return form(rec.lines, 'draw').filter((l) => !l.startsWith('@cv'));
}

const newSheet = () => {
  stubOn = false;
  const el = new M.UniversalSheetElement();
  stubOn = true;
  el.setAttribute('formula-locale', 'pt-BR');
  dom.document.body.appendChild(el);
  return el;
};

const W = 640, H = 360;
/** The region of the camera render (C4b): columns x rows. */
const CAM = [8, 14];
const STATES = [
  { z: 1, sx: 0, sy: 0, dpr: 1, sel: [1, 1, 1, 1, 1, 1] },
  { z: 1.75, sx: 41.5, sy: 17.25, dpr: 2, sel: [3, 3, 2, 2, 4, 5] },
  { z: 0.75, sx: 123, sy: 57, dpr: 1, sel: [2, 2, 2, 2, 2, 2] },
  { z: 1.25, sx: 700.5, sy: 333, dpr: 1.5, sel: [5, 7, 5, 7, 8, 9] },
];

const results = [];
const w = new CaptureWriter(['Sheet viewport op stream (frontier C4): see core/crates/uo-xlsx/js/capture-viewport.mjs']);
w.meta('ts-root-commit', h.commit);
w.meta('ts-root-state', C.tsClean(root));
w.meta('tz', process.env.TZ);
const lines = new Map();

function viewFacets(key, bytes) {
  const el = newSheet();
  el.shouldRecalcOnOpen = () => false;
  // the drawing phases go to a recorder of their own (C4b): the quadrant clips around them stay in the main stream
  const calls = [];
  const paintDrawings = el.paintDrawings.bind(el);
  el.paintDrawings = (_ctx, sx, sy, view) => { calls.push(recordCall((c) => paintDrawings(c, sx, sy, view))); };
  const paintChartsheet = el.paintChartsheet.bind(el);
  el.paintChartsheet = (_ctx, v, ww, hh) => { calls.push(recordCall((c) => paintChartsheet(c, v, ww, hh))); };
  try { el.loadXlsx(bytes); } catch (e) { w.set(key, 'open', `err:${C.oneLine(e?.message ?? e)}`); el.remove(); return; }
  w.set(key, 'open', 'ok');
  // pictures: decoded at once as hashed fake images (the Slides harness's `rec.image`), as the shell's decode would give them
  const layer = el.drawingLayer;
  if (layer) {
    // the open already read the active sheet's objects (the fake DOM never decodes their images): read them again
    layer.images.clear();
    layer.parsed = new WeakMap();
    layer.warm = function warm(ref) {
      if (this.images.has(ref.part)) return;
      const d = imageSize(ref.bytes) ?? { w: 96, h: 96 };
      this.images.set(ref.part, { img: { width: d.w, height: d.h, naturalWidth: d.w, naturalHeight: d.h, __h: sha1(ref.bytes) }, ready: true });
    };
  }
  // the shape font stacks the element gives (the replay of the Rust list uses the same)
  const stacks = new Map();
  const shapeFontStack = el.shapeFontStack;
  el.shapeFontStack = (f, b = false, i = false) => { const st = shapeFontStack(f, b, i); stacks.set(`${f}|${b ? 1 : 0}|${i ? 1 : 0}`, st); return st; };
  const keep = el.activeSheet;
  const views = el.views ?? [];
  // the element's own canvas: sized, and its context replaced by the recorder
  Object.defineProperty(el.canvas, 'clientWidth', { value: W, configurable: true });
  Object.defineProperty(el.canvas, 'clientHeight', { value: H, configurable: true });
  el.paintScrollbars = () => {};
  el.spillRangeAt = () => null;
  const all = [];
  const drawText = [];
  views.forEach((v, i) => {
    el.selectSheet(i);
    STATES.forEach((st, k) => {
      el.zoom = st.z; el.scrollX = st.sx; el.scrollY = st.sy;
      el.sel = { col: st.sel[0], row: st.sel[1] };
      el.range = { c1: st.sel[2], r1: st.sel[3], c2: st.sel[4], r2: st.sel[5] };
      el.recalcGrid();
      dom.window.devicePixelRatio = st.dpr;
      globalThis.window = dom.window;
      const rec = recorder();
      el.canvas.getContext = () => rec.ctx;
      calls.length = 0;
      el.paint();
      const cl = canon(rec.ev);
      const dtext = calls.map((c) => ['#call', ...c].join('\n')).join('\n');
      w.set(key, `d${i}@${k}`, dig(dtext));
      w.set(key, `dc${i}@${k}`, String(calls.length));
      if (calls.length) drawText.push(`# ${i}@${k}`, dtext);
      const text = cl.join('\n');
      w.set(key, `v${i}@${k}`, dig(text));
      w.set(key, `n${i}@${k}`, String(cl.length));
      all.push(`# ${i}@${k}`, ...cl);
    });
    // C4b: the camera render (`renderRegionToCanvas`) of the first CAM columns x rows as state 4 (a right-to-left sheet mirrored)
    if (!v.worksheet.chartsheet) {
      const rec = recorder();
      calls.length = 0;
      const cv = { width: 0, height: 0, getContext: () => rec.ctx };
      const wh = el.renderRegionToCanvas(i, cv, CAM[0], CAM[1]);
      const cl = canon(rec.ev);
      w.set(key, `v${i}@4`, dig(cl.join('\n')));
      w.set(key, `n${i}@4`, String(cl.length));
      w.set(key, `w${i}@4`, `${wh.w},${wh.h}`);
      const dtext = calls.map((c) => ['#call', ...c].join('\n')).join('\n');
      w.set(key, `d${i}@4`, dig(dtext));
      w.set(key, `dc${i}@4`, String(calls.length));
      all.push(`# ${i}@4`, ...cl);
      if (calls.length) drawText.push(`# ${i}@4`, dtext);
    }
  });
  if (LINES_DIR) lines.set(key, all.join('\n'));
  if (LINES_DIR && drawText.length) lines.set(`${key}#drawings`, drawText.join('\n'));
  w.set(key, 'stacks', JSON.stringify([...stacks].sort((x, y) => (x[0] < y[0] ? -1 : 1))));
  el.selectSheet(keep);
  el.remove();
}

const t0 = performance.now();
if (!BIG) {
  for (let i = 0; i < GROUPS.x1.count; i++) { const key = groupKey('x1', i); viewFacets(key, new Uint8Array(readFileSync(join(C.capturesDir, 'workbooks', 'x1', workbookFile('x1', i))))); results.push(key); }
  for (let i = 0; i < PAINT_GROUPS.paint1.count; i++) { const key = paintKey('paint1', i); viewFacets(key, new Uint8Array(buildPaintWorkbook('paint1', i).bytes)); results.push(key); }
  for (let i = 0; i < CF_GROUPS.cf1.count; i++) { const key = cfKey('cf1', i); viewFacets(key, new Uint8Array(readFileSync(join(C.capturesDir, 'workbooks', 'cf1', cfFile('cf1', i))))); results.push(key); }
  for (const rel of C.trackedSamples()) { if (/giant/.test(rel)) continue; const key = `samples/${rel}`; viewFacets(key, C.readSample(rel)); results.push(key); }
  // C4b: the drawing workbooks (committed, rewritten from the generator: `--check` would compare them byte for byte)
  const drDir = join(C.capturesDir, 'workbooks', 'dr1');
  mkdirSync(drDir, { recursive: true });
  for (let i = 0; i < DRAW_GROUPS.dr1.count; i++) {
    const b = buildDrawingWorkbook('dr1', i);
    writeFileSync(join(drDir, b.name), b.bytes);
    const key = drawKey('dr1', i); viewFacets(key, new Uint8Array(b.bytes)); results.push(key);
  }
} else {
  const base = join(C.bigDir, 'workbooks');
  for (let i = 0; i < GROUPS['x1-big'].count; i++) {
    const f = join(base, 'x1-big', workbookFile('x1-big', i));
    if (!existsSync(f)) continue;
    const key = groupKey('x1-big', i); viewFacets(key, new Uint8Array(readFileSync(f))); results.push(key);
  }
  const drBig = join(base, 'dr-big');
  mkdirSync(drBig, { recursive: true });
  for (let i = 0; i < DRAW_GROUPS['dr-big'].count; i++) {
    const b = buildDrawingWorkbook('dr-big', i);
    writeFileSync(join(drBig, b.name), b.bytes);
    const key = drawKey('dr-big', i); viewFacets(key, new Uint8Array(b.bytes)); results.push(key);
  }
  for (const [g, n, name] of [['paint-big', PAINT_GROUPS['paint-big']?.count ?? 0, (i) => `paint-big-${String(i).padStart(4, '0')}.xlsx`], ['cf-big', CF_GROUPS['cf-big']?.count ?? 0, (i) => cfFile('cf-big', i)]]) {
    for (let i = 0; i < n; i++) {
      const f = join(base, g, name(i));
      if (!existsSync(f)) continue;
      const key = `gen/${g}/${String(i).padStart(4, '0')}`; viewFacets(key, new Uint8Array(readFileSync(f))); results.push(key);
    }
  }
}

mkdirSync(join(outPath, '..'), { recursive: true });
w.write(outPath);
if (LINES_DIR) {
  mkdirSync(LINES_DIR, { recursive: true });
  for (const [key, text] of lines) writeFileSync(join(LINES_DIR, `${key.replace(/[\\/]/g, '_')}.txt.gz`), gzipSync(text));
}
console.log(`sheet-viewport: ${results.length} workbooks in ${((performance.now() - t0) / 1000).toFixed(1)} s -> ${outPath}`);
