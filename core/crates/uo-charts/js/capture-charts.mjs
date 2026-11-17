// Parity capture of the frozen TS chart engine (frontier C-0 of `core/docs/modules/uo-charts.md`): for every chart (a generated
// `c1a` chart or a chart part of the tracked samples) it records, against the frozen `shared/charts/dist` and the two host
// bridges (slides-core `chart-engine`, sheet-core `chart`), in the uo-testkit capture format:
//
//   input     len:sha256 of the chart XML (generated charts also carry `seed`, `feat`, `src`)
//   profile   `<host>/<fileFormat>/<theme>`  (host: ppt = Slides bridge, xl = Sheet bridge)
//   parse     `ok` | `err:<message>`
//   model     len:sha256[0..32] of the canonical JSON of the ChartSpace (canon-json.mjs; `themeFill` is a boolean; Map and Set
//             as `$map` / `$set`)           dropped   what the canonical JSON dropped (`what:count,...`)
//   host      n:sha256[0..16] of the host call log of the PARSE (call order: `kind\tkey\tanswer` lines, see below)
//   hostx     the same over the calls made while painting (`themeFill`), deduplicated
//   ops.<view>.<sel>      lines:sha1[0..16] of the `draw` form of the op stream of `paintChartSpace` over record-ctx.mjs
//   ask.<view>.<sel>      n:sha256[0..16] of the sorted distinct measureText keys (`<font>\t<JSON text>`)
//   bounds.<view>.<sel>   canonical JSON of `chartPartBounds` (ppt profile only): {"h":..,"w":..,"x":..,"y":..} or null
//   lay.<view>[.<method>] n:sha256[0..16] of the layout facets (patched in-memory copy of render.js; sidecar .layout.jsonl.gz)
//
// Views: `a` = (0, 0, 480 x 300), 4/3 px per pt; `b` = (17.5, 9.25, 360.5 x 240.25), 0.8833 px per pt.
// Selections: full (no `sel`), bg ({bg, data: null}), none ({bg: false, data: null}), s0 (series 0), c1 (category 1),
// p02 (series 0, category 2), not-s0 (everything but series 0, no bg). Every chart: view a full + bg, view b full; every 3rd chart
// (and every sample chart) all selections on view a, and bounds for all of them.
//
// Files (stem `tests/captures/charts-<group>`): .capture, .data.jsonl.gz (inputs + host log), .ops.gz, .measures.tsv.gz,
// .layout.jsonl.gz. The host log key of `color` / `blip` is the canonical JSON of the element ({"attrs":{sorted},"children":[..],
// "name":..}); of `fill` it is `<idx>|<phClr>`; the answer is the canonical JSON of the value or `null`.
//
//   node core/crates/uo-charts/js/capture-charts.mjs --gen c1a
//   node core/crates/uo-charts/js/capture-charts.mjs --samples
//   node core/crates/uo-charts/js/capture-charts.mjs --diff
//   options: --out <stem>  --only <substring of the key>  --max <n>  --count <n> (generated charts)  --detail <file.json.gz>
//            --ts-root <dir> (default UO_TS_ROOT, else the 0.9.2 reference)
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';
import { CaptureWriter, fffd } from '../../uo-testkit/js/testkit.mjs';
import { canonJson, textDigest } from '../../uo-pptx/js/lib/canon-json.mjs';
import { args, oneLine, readSample, repoDir, trackedSamples, tsCommit, tsRoot } from '../../uo-pptx/js/lib/common.mjs';
import { createRecorder } from '../../uo-pptx/js/lib/record-ctx.mjs';
import { digest as drawDigest } from '../../uo-pptx/js/lib/opstream.mjs';
import { imageSize } from '../../uo-pptx/js/lib/ts-paint.mjs';
import { createMeasurer } from './lib/font-measure.mjs';
import { loadTs, parseChartPpt, parseChartXl, syntheticPptTheme } from './lib/ts-charts.mjs';
import { SEED_BASE, C1A_COUNT, buildChart, chartKey } from './gen/charts.mjs';
import { runDiff } from './lib/diff.mjs';
import { execFileSync } from 'node:child_process';

const here = fileURLToPath(new URL('.', import.meta.url));
const crateDir = resolve(here, '..');
export const capturesDir = join(crateDir, 'tests', 'captures');

const a = args();
const root = tsRoot(a);
const ts = await loadTs(root);
const only = a.opt('--only');
const max = Number(a.opt('--max', '1e9'));
// the op streams and the layout lines are only diff aids (the facets carry their digests): the first N charts are kept
const opsMax = Number(a.opt('--ops-max', '80'));
const sha256 = (s, n = 16) => createHash('sha256').update(s).digest('hex').slice(0, n);
const setDigest = (keys) => { const k = [...keys].sort(); return `${k.length}:${sha256(k.join('\n'))}`; };

const PPT_STACK = (f) => `'${f}', 'Aptos', Calibri, 'Liberation Sans', Arial, sans-serif`;
const XL_STACK = (f) => `'${f}', Calibri, 'Liberation Sans', Arial, sans-serif`;
const VIEWS = {
  a: { x: 0, y: 0, w: 480, h: 300, ppt: 4 / 3, cw: 480, ch: 300 },
  b: { x: 17.5, y: 9.25, w: 360.5, h: 240.25, ppt: 0.8833, cw: 400, ch: 270 },
};
const SELS = {
  full: undefined,
  bg: { bg: true, data: null },
  none: { bg: false, data: null },
  s0: { bg: false, data: (s) => s === 0 },
  c1: { bg: false, data: (_s, c) => c === 1 },
  p02: { bg: false, data: (s, c) => s === 0 && c === 2 },
  'not-s0': { bg: false, data: (s) => s !== 0 },
};

// ------------------------------------------------------------------------------------------------ themes
const pptThemes = new Map();
function pptTheme(name) {
  if (name === 'none') return undefined;
  if (!pptThemes.has(name)) {
    const t = syntheticPptTheme(ts.slides.parseXml);
    if (name === 'alt') {
      Object.assign(t.colors, { accent1: '#C0504D', accent2: '#9BBB59', accent3: '#8064A2', accent4: '#4BACC6', accent5: '#F79646', accent6: '#2C4D75', tx1: '#1F1F1F', bg1: '#F2F2F2' });
      t.fonts = { major: 'Cambria', minor: 'Arial' };
    }
    pptThemes.set(name, t);
  }
  return pptThemes.get(name);
}
function xlTheme(name) {
  if (name === 'none') return undefined;
  const fill = (xml) => ts.sheet.parseXml(xml);
  return {
    colors: [], byName: { dk1: '#000000', lt1: '#FFFFFF', dk2: '#1F497D', lt2: '#EEECE1', accent1: '#4F81BD', accent2: '#C0504D', accent3: '#9BBB59', accent4: '#8064A2', accent5: '#4BACC6', accent6: '#F79646', hlink: '#0000FF', folHlink: '#800080' },
    fonts: { major: 'Cambria', minor: 'Calibri' }, lineWidths: [0.5, 1, 2],
    fillStyles: [
      fill('<a:solidFill xmlns:a="x"><a:schemeClr val="phClr"/></a:solidFill>'),
      fill('<a:gradFill xmlns:a="x"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="50000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="85000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>'),
      fill('<a:gradFill xmlns:a="x"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="60000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="70000"/></a:schemeClr></a:gs></a:gsLst><a:path path="circle"><a:fillToRect l="50000" t="50000" r="50000" b="50000"/></a:path></a:gradFill>'),
    ],
  };
}

// ------------------------------------------------------------------------------------------------ inputs
/** { key, host, themeName|theme, fileFormat, noTheme, xml, userShapesXml, images: {rid: {part, bytes, mime}}, extra } */
function* generated(group, count) {
  for (let i = 0; i < count; i++) {
    const c = buildChart(SEED_BASE + i, i);
    yield { key: chartKey(group, i), host: c.host, themeName: c.themeName, theme: c.host === 'ppt' ? pptTheme(c.themeName) : xlTheme(c.themeName), noTheme: c.host === 'xl' && c.themeName === 'none', fileFormat: c.fileFormat, xml: c.xml, userShapesXml: c.userShapesXml, images: c.images, extra: { seed: SEED_BASE + i, feat: c.features.join(','), src: c.name } };
  }
}

/** every object with a string `chartXml` below `v` (shapes, groups, graphic frames) */
function collectChartShapes(v, out = [], seen = new Set()) {
  if (!v || typeof v !== 'object' || v instanceof Uint8Array || seen.has(v)) return out;
  seen.add(v);
  if (typeof v.chartXml === 'string') out.push(v);
  for (const k of Object.keys(v)) if (k !== 'chartTheme' && k !== 'chartImages') collectChartShapes(v[k], out, seen);
  return out;
}

function* sampleCharts() {
  const seenXml = new Map();
  for (const rel of trackedSamples()) {
    const src = `samples/${rel}`;
    let pkg, pres;
    try { pkg = ts.slides.PptxPackage.load(readSample(rel)); pres = ts.slides.readPresentation(pkg); } catch { continue; }
    let n = 0;
    for (let i = 0; i < pres.slides.length; i++) {
      let slide;
      try { slide = ts.slides.readSlide(pkg, pres.slides[i].target); } catch { continue; }
      for (const sh of collectChartShapes(slide)) {
        const sig = `${sh.chartXml}\u0000${Object.values(sh.chartImages ?? {}).map((r) => r.part).join('|')}\u0000${sh.chartFormat}`;
        if (seenXml.has(sig)) { seenXml.get(sig).extra.also.push(`${src}@s${i}`); continue; }
        const c = { key: `${src}#c${n++}`, host: 'ppt', themeName: `deck:${rel}`, theme: sh.chartTheme, fileFormat: sh.chartFormat, xml: sh.chartXml, images: sh.chartImages ?? {}, extra: { src: `${src}@s${i}`, also: [] } };
        seenXml.set(sig, c);
        yield c;
      }
    }
  }
  const { XlsxPackage } = ts.sheet;
  for (const rel of execFileSync('git', ['-C', repoDir, 'ls-files', 'sheet/samples'], { encoding: 'utf8' }).split('\n').map((s) => s.trim()).filter((s) => /\.xlsx$/i.test(s))) {
    const name = rel.slice('sheet/samples/'.length);
    let pkg;
    try { pkg = XlsxPackage.load(new Uint8Array(readFileSync(join(repoDir, rel)))); } catch { continue; }
    const parts = pkg.partNames().filter((p) => /^xl\/charts\/(chart|chartEx)[^/]*\.xml$/i.test(p)).sort();
    if (!parts.length) continue;
    const noTheme = !pkg.partNames().some((p) => /^xl\/theme\/[^/]+\.xml$/i.test(p));
    const theme = noTheme ? undefined : ts.sheet.readTheme(pkg);
    const fileFormat = ts.sheet.chartFileFormat(pkg);
    for (const p of parts) {
      let userShapesXml;
      const relsXml = pkg.getPartText(p.replace(/([^/]+)$/, '_rels/$1.rels'));
      if (relsXml) {
        const m = /<Relationship [^>]*Type="[^"]*\/chartUserShapes"[^>]*>/.exec(relsXml);
        const t = m && /Target="([^"]+)"/.exec(m[0]);
        if (t) userShapesXml = pkg.getPartText(pkg.resolveRelTarget(p, t[1]));
      }
      yield { key: `samples/${name}#${p.slice('xl/charts/'.length)}`, host: 'xl', themeName: noTheme ? 'none' : `book:${name}`, theme, noTheme, fileFormat, xml: pkg.getPartText(p), userShapesXml, images: {}, extra: { src: `samples/${name}!${p}` } };
    }
  }
}

// ------------------------------------------------------------------------------------------------ the run state
const measurer = await createMeasurer(root);
const globalMeasures = new Map(); // `"font"\t"text"` -> { w, asc, desc }
let asks; // Set of keys of the current run
const measure = measurer.measure((font, text, w, asc, desc) => {
  const k = `${JSON.stringify(font)}\t${JSON.stringify(text)}`;
  asks.add(k);
  const e = globalMeasures.get(k);
  if (!e) globalMeasures.set(k, { w, asc, desc });
  else if (e.w !== w) throw new Error(`non-deterministic measure ${k}`);
});

// ------------------------------------------------------------------------------------------------ layout facets (patched copy)
const LAYOUT_METHODS = ['layoutTitle', 'layoutLegend', 'legendEntries', 'prepValueAxis', 'prepCatAxis', 'measureAxis', 'plotRect', 'dateLabels', 'valueLabels', 'trendCurve', 'hierarchy'];
const patched = await ts.patchedRender();
let layoutSink = null; // { calls: [], refs: Map }
for (const m of LAYOUT_METHODS) {
  const orig = patched.Renderer.prototype[m];
  if (typeof orig !== 'function') throw new Error(`Renderer.${m} not found in the frozen render.js`);
  patched.Renderer.prototype[m] = function (...argv) {
    const s = layoutSink;
    if (!s) return orig.apply(this, argv);
    const seq = s.calls.length;
    s.calls.push(null);
    let res, thrown;
    try { res = orig.apply(this, argv); } catch (e) { thrown = e; }
    s.calls[seq] = { m, seq, args: argv.map((v) => plain(v, s.refs)), res: thrown ? { $throw: String(thrown?.message ?? thrown) } : plain(res, s.refs) };
    if (thrown) throw thrown;
    return res;
  };
}
/** serializable copy: model objects (groups, series, axes, trendlines) become `{$ref}`; functions are dropped; depth-limited */
function plain(v, refs, depth = 0) {
  if (v === undefined) return { $undef: 1 };
  if (v === null || typeof v !== 'object') return typeof v === 'function' ? { $fn: 1 } : v;
  if (refs.has(v)) return { $ref: refs.get(v) };
  if (depth > 12) return { $deep: 1 };
  if (Array.isArray(v)) return v.map((x) => plain(x, refs, depth + 1));
  if (v instanceof Map) return { $map: [...v].map(([k, x]) => [plain(k, refs, depth + 1), plain(x, refs, depth + 1)]) };
  if (v instanceof Set) return { $set: [...v].map((x) => plain(x, refs, depth + 1)) };
  const o = {};
  for (const k of Object.keys(v)) if (typeof v[k] !== 'function') o[k] = plain(v[k], refs, depth + 1);
  return o;
}
function refsOf(cs) {
  const refs = new Map();
  cs.groups.forEach((g, gi) => {
    refs.set(g, `g${gi}`);
    g.series.forEach((s, si) => { refs.set(s, `g${gi}.s${si}`); (s.trendlines ?? []).forEach((t, ti) => refs.set(t, `g${gi}.s${si}.t${ti}`)); });
  });
  cs.axes.forEach((x, i) => refs.set(x, `ax${i}`));
  return refs;
}

// ------------------------------------------------------------------------------------------------ recording one paint
const nodeKey = (el) => canonJson(el);

function imageTable(ch) {
  const t = new Map();
  for (const [rid, r] of Object.entries(ch.images)) {
    const d = imageSize(r.bytes) ?? { w: 96, h: 96 };
    const dpi = ts.image.imageDpi(r.bytes);
    t.set(r.part, { rid, part: r.part, w: d.w, h: d.h, ...(dpi ? { wPt: (d.w * 72) / dpi.x, hPt: (d.h * 72) / dpi.y } : {}), sha: createHash('sha1').update(r.bytes).digest('hex').slice(0, 12), bytes: r.bytes });
  }
  return t;
}

function paintOptions(ch, view, selName, image) {
  const sel = SELS[selName];
  const base = ch.host === 'ppt'
    ? { pxPerPt: view.ppt, defaultAreaFill: 'none', cssFont: PPT_STACK }
    : { pxPerPt: view.ppt, defaultAreaFill: 'bg1', defaultAreaLine: 'auto2010', excelAxisDensity: true, cssFont: XL_STACK };
  return { ...base, ...(sel ? { sel } : {}), ...(image ? { image } : {}) };
}

/** one op-stream recording: { text, lines, asks, error } */
async function recordOps(ch, space, viewName, selName, imgs, render) {
  const view = VIEWS[viewName];
  asks = new Set();
  const rec = createRecorder({ width: view.cw, height: view.ch, caps: { roundRect: true, strictFont: true }, measure });
  const resolved = new Map();
  const image = ch.host === 'ppt' && imgs.size ? (part) => {
    const m = imgs.get(part);
    if (!m) return undefined;
    if (!resolved.has(part)) resolved.set(part, { src: rec.image(m.bytes, m.w, m.h), w: m.w, h: m.h, ...(m.wPt !== undefined ? { wPt: m.wPt, hPt: m.hPt } : {}) });
    return resolved.get(part);
  } : undefined;
  rec.install();
  let error;
  try { render.paintChartSpace(rec.ctx, space, view.x, view.y, view.w, view.h, paintOptions(ch, view, selName, image)); } catch (e) { error = String(e?.message ?? e); } finally { rec.uninstall(); }
  return { text: rec.text(), asks, error };
}

function recordBounds(ch, space, viewName, selName, imgs) {
  const view = VIEWS[viewName];
  asks = new Set();
  const rec = createRecorder({ width: view.cw, height: view.ch, caps: { roundRect: true, strictFont: true }, measure });
  rec.install();
  try {
    const box = ts.slides.chartPartBounds(rec.ctx, { space }, view.x, view.y, view.w, view.h, view.ppt, { bg: SELS[selName]?.bg ?? true, data: SELS[selName] ? SELS[selName].data : () => true });
    return box ? canonJson(box) : 'null';
  } catch (e) { return `err:${oneLine(e?.message ?? e)}`; } finally { rec.uninstall(); }
}

function recordLayout(ch, space, viewName, imgs) {
  const view = VIEWS[viewName];
  asks = new Set();
  const rec = createRecorder({ width: view.cw, height: view.ch, caps: { roundRect: true, strictFont: true }, measure });
  layoutSink = { calls: [], refs: refsOf(space) };
  rec.install();
  try { patched.paintChartSpace(rec.ctx, space, view.x, view.y, view.w, view.h, paintOptions(ch, view, 'full', undefined)); } catch { /* the op facet carries the error */ } finally { rec.uninstall(); }
  const calls = layoutSink.calls.filter(Boolean);
  layoutSink = null;
  return calls;
}

// ------------------------------------------------------------------------------------------------ main loop
const writer = new CaptureWriter(['Chart parity capture of the frozen TS charts engine (C-0); see js/capture-charts.mjs and core/docs/modules/uo-charts.md "C-0 — harness"']);
writer.meta('ts-root-commit', tsCommit(root));
writer.meta('views', 'a=0,0,480x300@4/3 ppt; b=17.5,9.25,360.5x240.25@0.8833');
writer.meta('measure', 'real advance widths (hmtx) and GPOS/kern pairs of the open fonts, units * px / upem; font box hhea ascender/descender; substitutes calibri->carlito, cambria->caladea, times->liberation serif, courier->liberation mono, else liberation sans');
writer.meta('stream', 'record-ctx.mjs draw form (caps roundRect + strictFont); fonts: ppt stack `\'F\', \'Aptos\', Calibri, \'Liberation Sans\', Arial, sans-serif`, xl stack `\'F\', Calibri, \'Liberation Sans\', Arial, sans-serif`');

const streams = [], dataLines = [], layoutLines = [], detail = {};
const stats = { charts: 0, parseErr: 0, facets: 0, ops: 0, opsLines: 0, opsErr: 0, layoutCalls: 0, bounds: 0, boundsErr: 0, hostCalls: 0, drop: new Map() };
const t0 = Date.now();

async function runChart(ch) {
  if (only && !ch.key.includes(only)) return;
  if (stats.charts >= max) return;
  stats.charts++;
  const key = ch.key;
  const set = (f, v) => { writer.set(key, f, v); stats.facets++; };
  set('input', `${ch.xml.length}:${sha256(ch.xml, 32)}`);
  for (const [k, v] of Object.entries(ch.extra ?? {})) set(k, Array.isArray(v) ? v.join('|') || '-' : String(v));
  set('profile', `${ch.host}/${ch.fileFormat ?? '-'}/${ch.themeName}`);
  const log = [];
  let space, rootNode, hostObj;
  try {
    const r = ch.host === 'ppt'
      ? parseChartPpt(ts, { xml: ch.xml, theme: ch.theme, images: ch.images, fileFormat: ch.fileFormat }, log)
      : parseChartXl(ts, { xml: ch.xml, theme: ch.theme, noTheme: ch.noTheme, fileFormat: ch.fileFormat, userShapesXml: ch.userShapesXml }, log);
    space = r.space; rootNode = r.root; hostObj = r.host;
  } catch (e) { set('parse', `err:${oneLine(e?.message ?? e)}`); stats.parseErr++; return; }
  set('parse', 'ok');
  const parseLog = log.slice();
  const dropped = new Map();
  const model = canonJson({ ...space, themeFill: space.themeFill ? true : undefined }, dropped);
  set('model', textDigest(model));
  set('dropped', [...dropped].map(([k, n]) => `${k}:${n}`).sort().join(',') || '-');
  // the copy of the bridge must give the model of the real one
  const real = ch.host === 'ppt'
    ? ts.slides.parseChart(ch.xml, ch.theme, ch.images, ch.fileFormat).space
    : ts.sheet.parseChart(ch.xml, ch.theme, ch.fileFormat, { noTheme: ch.noTheme, ...(ch.userShapesXml ? { userShapes: ts.charts.canonicalNs(ts.sheet.parseXml(ch.userShapesXml)) } : {}) }).space;
  const realText = canonJson({ ...real, themeFill: real.themeFill ? true : undefined });
  if (realText !== model) throw new Error(`${key}: the bridge copy differs from the real parseChart`);
  if (a.opt('--detail')) detail[key] = model;
  const line = (e) => `${e.k}\t${e.key}\t${e.ans}`;
  set('host', setDigestOrdered(parseLog.map(line)));
  stats.hostCalls += parseLog.length;
  const imgs = imageTable(ch);

  // ---- op streams
  const all = (stats.charts - 1) % 3 === 0 || key.startsWith('samples/');
  const plan = [['a', 'full'], ['a', 'bg'], ['b', 'full']];
  if (all) for (const s of ['none', 's0', 'c1', 'p02', 'not-s0']) plan.push(['a', s]);
  if (all) plan.push(['b', 'bg']);
  const keep = stats.charts <= opsMax;
  if (keep) streams.push(`#chart ${key}\n`);
  for (const [v, s] of plan) {
    const render = await ts.freshRender();
    const r = await recordOps(ch, space, v, s, imgs, render);
    const d = await drawDigest(r.text);
    stats.ops++; stats.opsLines += d.lines;
    const view = VIEWS[v];
    if (keep) streams.push(`#view ${v} sel=${s} scale=${view.ppt} x=${view.x} y=${view.y} w=${view.w} h=${view.h} lines=${d.lines} sha=${d.sha}\n`);
    if (r.error) { if (keep) streams.push(`#error ${oneLine(r.error)}\n`); stats.opsErr++; }
    if (keep) streams.push(d.text);
    set(`ops.${v}.${s}`, r.error ? `err:${oneLine(r.error)}` : `${d.lines}:${d.sha}`);
    set(`ask.${v}.${s}`, setDigest(r.asks));
    if (ch.host === 'ppt' && !(v === 'b' && s === 'bg')) {
      const b = recordBounds(ch, space, v, s, imgs);
      set(`bounds.${v}.${s}`, b);
      stats.bounds++; if (b.startsWith('err:')) stats.boundsErr++;
    }
  }
  // ---- layout facets
  for (const v of all ? ['a', 'b'] : ['a']) {
    const calls = recordLayout(ch, space, v, imgs);
    stats.layoutCalls += calls.length;
    const lines = calls.map((c) => `${key}\t${v}\t${c.seq}\t${c.m}\t${canonJson({ args: c.args, res: c.res })}`);
    if (keep) layoutLines.push(...lines);
    set(`lay.${v}`, setDigestOrdered(lines.map((l) => l.split('\t').slice(2).join('\t'))));
    for (const m of LAYOUT_METHODS) {
      const ls = calls.filter((c) => c.m === m).map((c) => canonJson({ args: c.args, res: c.res }));
      if (ls.length) set(`lay.${v}.${m}`, setDigestOrdered(ls));
    }
  }
  // ---- the inputs + host log sidecar (paint-phase calls deduplicated)
  const seen = new Set(parseLog.map((e) => `${e.k}\t${e.key}`));
  const paintLog = [];
  for (const e of log.slice(parseLog.length)) { const k = `${e.k}\t${e.key}`; if (!seen.has(k)) { seen.add(k); paintLog.push(e); } }
  set('hostx', setDigestOrdered(paintLog.map(line)));
  dataLines.push(JSON.stringify({
    key, host: ch.host, fileFormat: ch.fileFormat ?? null, theme: ch.themeName, noTheme: !!ch.noTheme, xml: ch.xml, userShapes: ch.userShapesXml ?? null,
    images: [...imgs.values()].map((m) => ({ rid: m.rid, part: m.part, w: m.w, h: m.h, ...(m.wPt !== undefined ? { wPt: m.wPt, hPt: m.hPt } : {}), sha: m.sha })),
    fonts: hostObj.fonts ?? null, themeLn: hostObj.themeLineWidths ?? null, hostLog: parseLog, hostPaint: paintLog,
  }));
  for (const [k, n] of dropped) stats.drop.set(k, (stats.drop.get(k) ?? 0) + n);
}

/** order-sensitive digest of lines */
function setDigestOrdered(lines) { return `${lines.length}:${sha256(lines.join('\n'))}`; }

let group;
if (a.flag('--samples')) {
  group = 'samples';
  for (const c of [...sampleCharts()]) await runChart(c);
} else if (a.opt('--gen')) {
  group = a.opt('--gen');
  if (group !== 'c1a') throw new Error(`unknown group ${group}`);
  for (const c of generated(group, Number(a.opt('--count', String(C1A_COUNT))))) await runChart(c);
} else if (a.flag('--diff')) {
  group = 'diff';
} else {
  console.error('usage: capture-charts.mjs (--gen c1a | --samples | --diff) [--out stem] [--only s] [--max n] [--count n] [--detail file]');
  process.exit(2);
}

const stem = a.opt('--out', join(capturesDir, `charts-${group}`));
mkdirSync(dirname(stem), { recursive: true });
const gz = (text) => gzipSync(Buffer.from(text, 'utf8'), { level: 9 });
const sizes = {};
const put = (name, text) => { const b = gz(text); writeFileSync(`${stem}.${name}`, b); sizes[name] = b.length; };
if (group === 'diff') {
  const d = runDiff(ts, writer);
  put('cases.jsonl.gz', d.lines.join('\n') + '\n');
  Object.assign(stats, { diffCases: d.cases });
} else {
  put('ops.gz', streams.join(''));
  put('data.jsonl.gz', dataLines.join('\n') + '\n');
  const mt = [...globalMeasures].sort((x, y) => (x[0] < y[0] ? -1 : x[0] > y[0] ? 1 : 0)).map(([k, v]) => `${k}\t${v.w}\t${v.asc}\t${v.desc}`).join('\n');
  put('measures.tsv.gz', mt + '\n');
  put('layout.jsonl.gz', layoutLines.join('\n') + '\n');
  if (a.opt('--detail')) writeFileSync(a.opt('--detail'), gz(JSON.stringify(detail)));
}
writer.write(`${stem}.capture`);
const sec = ((Date.now() - t0) / 1000).toFixed(1);
console.log(`${stats.diffCases !== undefined ? `${stats.diffCases} differential cases; ` : ''}${stats.charts} charts (${stats.parseErr} parse errors), ${stats.facets} facets, ${stats.ops} op streams (${stats.opsLines} draw lines, ${stats.opsErr} paint errors), ` +
  `${stats.bounds} bounds (${stats.boundsErr} errors), ${stats.layoutCalls} layout calls, ${stats.hostCalls} parse host calls, ${globalMeasures.size} distinct measureText keys; ` +
  `${Object.entries(sizes).map(([k, v]) => `${k} ${v} B`).join(', ')}; ${sec} s -> ${stem}.*`);
