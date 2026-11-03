// Parity capture of the preset engine of uo-drawingml (phase 1 of stage 3): the frozen TS
// `presetPath`, `evalPreset`, `hasPresetDef`, `presetGuides`, `presetTextRect`, `presetOutline`,
// `customGuides` and `arcToSeg` (shared/drawingml, built in <ts-root>/shared/drawingml/dist) over
// the corpus below. Writes tests/captures/presets.capture, replayed by tests/preset_parity.rs.
//
//   node tools/capture-presets.mjs --ts-root <checkout of main>     (or UO_TS_ROOT)
//
// Options: --out <file>  write elsewhere (comparisons between platforms)
//
// Corpus (mirrored in tests/preset_parity.rs; every input is reproducible from this file, the
// capture lines carry only what the Rust side cannot derive: the guide names and the defaults of
// each preset, read from the TS):
//  - every preset of PRESET_DEFS plus the names presetPath draws by hand, unknown names, '',
//    undefined and Object.prototype names x 7 sizes x the adjust sets of `adjSets` (none, [],
//    defaults, each av perturbed by 0, -1, 1e5, 1e9, -1e9, NaN, +-Infinity and 1e308 as
//    positional, named, partial, reordered and duplicated lists, short and long lists, unknown
//    names): presetPath, evalPreset, presetGuides, presetTextRect, presetOutline;
//  - custom guides: 5,000 programs of the seeded generator `program` (all ops, unknown ops,
//    forward references, names that shadow the built-ins, '' and '0', odd tokens);
//  - arcToSeg: a grid including NaN and infinities.
//
// None of the facets calls Math.pow (sin, cos, tan, atan2 and sqrt only, all fdlibm in V8), so
// the file is the same on Node x64 Windows and linux-x64; the committed one is taken in WSL with
// the pow reference environment of the uo-jsnum gate anyway.
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Rng, Stream, importFrom, tsRoot } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const outIdx = argv.indexOf('--out');
const out = outIdx >= 0 ? argv[outIdx + 1] : join(here, '..', 'tests', 'captures', 'presets.capture');
const root = tsRoot(argv);
const dml = await importFrom(root, 'shared/drawingml/dist/index.js');
const { PRESET_DEFS } = await importFrom(root, 'shared/drawingml/dist/preset-defs.js');

const canon = (v) => (v !== v ? NaN : v);
const f = (s, v) => s.f64(canon(v));
const summary = (s) => { const n = s.items; return `${n}:${s.finish().short}`; };

// ── the corpus (mirrored in tests/preset_parity.rs) ─────────────────────────────────────────────

const SIZES = [[200, 100], [100, 200], [100, 100], [0, 0], [1, 1e6], [21600, 21600], [13.37, 0.5]];
const PERTURB = [0, -1, 1e5, 1e9, -1e9, NaN, Infinity, -Infinity, 1e308];
const BUILTINS = [
  'l', 't', 'r', 'b', 'w', 'h', 'hc', 'vc', 'ss', 'ls', 'cd2', 'cd4', 'cd8', '3cd4', '3cd8', '5cd8', '7cd8',
  ...[2, 3, 4, 5, 6, 8, 10, 12, 16, 32].flatMap((d) => [`wd${d}`, `hd${d}`, `ssd${d}`]),
];
const PROBE_NAMES = ['nope', '', '0', 'constructor', '__proto__'];
const PROBE_TOKENS = ['0', 'abc', '0x10', 'l', ' 7 ', '1e400', 'Infinity', '-5', '', '1_0'];
const HAND_DRAWN = [
  'frame', 'ellipse', 'roundRect', 'triangle', 'rtTriangle', 'diamond', 'parallelogram', 'trapezoid', 'pentagon', 'hexagon', 'star5',
  'rightArrow', 'leftArrow', 'upArrow', 'downArrow', 'chevron', 'pie', 'line', 'straightConnector1', 'bentConnector2', 'bentConnector3',
  'bentConnector4', 'curvedConnector2', 'curvedConnector3', 'curvedConnector4', 'curvedConnector5', 'arc', 'blockArc', 'octagon',
  'donut', 'moon', 'heart', 'lightningBolt', 'sun', 'can', 'cube', 'rect',
  'actionButtonBlank', 'actionButtonHome', 'actionButtonHelp', 'actionButtonInformation', 'actionButtonBackPrevious',
  'actionButtonForwardNext', 'actionButtonBeginning', 'actionButtonEnd', 'actionButtonReturn', 'actionButtonDocument',
  'actionButtonSound', 'actionButtonMovie',
];
const EXTRA_NAMES = [
  ['undef', undefined], ['empty', ''], ['nope', 'nope'], ['constructor', 'constructor'], ['proto', '__proto__'], ['toString', 'toString'],
  ['hasOwnProperty', 'hasOwnProperty'], ['valueOf', 'valueOf'], ['Frame', 'Frame'], ['actionButton', 'actionButton'],
  ['actionButtonNone', 'actionButtonNone'], ['rect', 'rect'], ['space', ' rect'],
];

function adjSets(names, defs) {
  const sets = [];
  const add = (adj, nm) => sets.push([adj, nm]);
  add(undefined, undefined);
  add([], undefined);
  add([], []);
  add(defs.slice(), undefined);
  add(defs.slice(), names.slice());
  for (let i = 0; i < names.length; i++) {
    for (const p of PERTURB) {
      const d = defs.slice();
      d[i] = p;
      add(d, undefined);
      add(d.slice(), names.slice());
      add([p], [names[i]]);
      add(d.slice().reverse(), names.slice().reverse());
      add([p, defs[i]], [names[i], names[i]]);
    }
  }
  for (const p of PERTURB) {
    add([p], undefined);
    add([p], ['adj']);
    add([...defs, p, 7], undefined);
  }
  add([1, 2, 3], ['adj']);
  add([1], ['nope']);
  return sets;
}

// ── encoders (mirrored in the Rust test) ────────────────────────────────────────────────────────

function segs(s, subs) {
  s.u32(subs.length);
  for (const sub of subs) {
    s.u32(sub.length);
    for (const g of sub) {
      s.tag(g.c);
      switch (g.c) {
        case 'M': case 'L': f(s, g.x); f(s, g.y); break;
        case 'C': f(s, g.x1); f(s, g.y1); f(s, g.x2); f(s, g.y2); f(s, g.x); f(s, g.y); break;
        case 'Q': f(s, g.x1); f(s, g.y1); f(s, g.x); f(s, g.y); break;
        case 'E': f(s, g.cx); f(s, g.cy); f(s, g.rx); f(s, g.ry); f(s, g.a0); f(s, g.a1); s.bool(!!g.ccw); break;
        case 'Z': break;
        default: throw new Error(`segment ${g.c}`);
      }
    }
  }
}

function encPath(s, p) {
  switch (p.kind) {
    case 'poly': s.tag('P'); s.u32(p.points.length); for (const q of p.points) { f(s, q.x); f(s, q.y); } break;
    case 'line': s.tag('L'); f(s, p.x1); f(s, p.y1); f(s, p.x2); f(s, p.y2); break;
    case 'pie': s.tag('I'); f(s, p.startDeg); f(s, p.endDeg); break;
    case 'rect': s.tag('R'); break;
    case 'ellipse': s.tag('E'); break;
    case 'roundRect': s.tag('O'); f(s, p.r); break;
    case 'path':
      s.tag('A');
      s.bool(p.fillRule === 'evenodd');
      segs(s, p.subs);
      s.bool(p.subLum !== undefined);
      if (p.subLum) { s.u32(p.subLum.length); for (const v of p.subLum) { if (v === null) s.tag('n'); else { s.tag('f'); f(s, v); } } }
      s.bool(p.subStroke !== undefined);
      if (p.subStroke) { s.u32(p.subStroke.length); for (const v of p.subStroke) s.bool(v); }
      break;
    default: throw new Error(`kind ${p.kind}`);
  }
}

function encEval(s, g) {
  if (g === null) { s.tag('n'); return; }
  s.tag('g');
  segs(s, g.subs);
  s.u32(g.fill.length);
  for (const v of g.fill) { if (v === null) s.tag('n'); else { s.tag('f'); f(s, v); } }
  s.u32(g.stroke.length);
  for (const v of g.stroke) s.bool(v);
}

function encGuides(s, m, names) {
  if (m === null) { s.tag('n'); return; }
  s.tag('m');
  s.u32(m.size);
  for (const n of names) { const v = m.get(n); if (v === undefined) s.tag('u'); else { s.tag('v'); f(s, v); } }
}

function encRect(s, r) {
  if (r === null) { s.tag('n'); return; }
  s.tag('r'); f(s, r.l); f(s, r.t); f(s, r.r); f(s, r.b);
}

function encOutline(s, o) {
  s.tag('o');
  segs(s, o.subs);
  s.u32(o.fill.length); for (const v of o.fill) s.bool(v);
  s.u32(o.stroke.length); for (const v of o.stroke) s.bool(v);
}

const attempt = (s, fn) => { try { fn(); } catch { s.tag('X'); } };

// ── presets ─────────────────────────────────────────────────────────────────────────────────────

const cap = new CaptureWriter([
  'uo-drawingml parity capture of the preset engine: written by core/crates/uo-drawingml/tools/capture-presets.mjs, replayed by tests/preset_parity.rs.',
  'preset/<name>: av = adjust names, avd = their defaults at 100x100, guides = av and gd names; path|eval|guides|rect|outline.s<N> = items:digest over every adjust set at size N (SIZES of the script).',
  'extra/<label>: the same for names without a definition; has/all = hasPresetDef over every name; custom/<k>: 250 seeded programs each; arc/grid: arcToSeg.',
]);
cap.meta('ts-dist', 'shared/drawingml/dist (frozen main, 0.9.x)');

const parseNames = (raw) => {
  const [av = '', gd = ''] = raw.split('|');
  const list = (s) => (s ? s.split(';').map((e) => e.slice(0, e.indexOf('=')).trim()) : []);
  return { av: list(av), all: [...new Set([...list(av), ...list(gd)])] };
};

function runPreset(key, name, avNames, defs, guideNames) {
  const sets = adjSets(avNames, defs);
  SIZES.forEach(([w, h], si) => {
    const sp = new Stream(), se = new Stream(), sg = new Stream(), sr = new Stream(), so = new Stream();
    sets.forEach(([adj, nm], k) => {
      for (const s of [sp, se, sg, sr, so]) s.u32(k);
      attempt(sp, () => encPath(sp, dml.presetPath(name, w, h, adj, nm)));
      attempt(se, () => (name === undefined ? se.tag('u') : encEval(se, dml.evalPreset(name, w, h, adj, nm))));
      attempt(sg, () => (name === undefined ? sg.tag('u') : encGuides(sg, dml.presetGuides(name, w, h, adj, nm), guideNames)));
      attempt(sr, () => encRect(sr, dml.presetTextRect(name, w, h, adj, nm)));
      attempt(so, () => {
        if (nm === undefined) { encOutline(so, dml.presetOutline(name, w, h, undefined)); return; }
        if (adj === undefined || nm.length > adj.length) { so.tag('s'); return; }
        const obj = {};
        nm.forEach((n, i) => { obj[n] = adj[i]; });
        encOutline(so, dml.presetOutline(name, w, h, obj));
      });
    });
    cap.set(key, `path.s${si}`, summary(sp));
    cap.set(key, `eval.s${si}`, summary(se));
    cap.set(key, `guides.s${si}`, summary(sg));
    cap.set(key, `rect.s${si}`, summary(sr));
    cap.set(key, `outline.s${si}`, summary(so));
  });
  const sh = new Stream();
  sh.bool(dml.hasPresetDef(name));
  cap.set(key, 'has', summary(sh));
}

const names = Object.keys(PRESET_DEFS);
for (const name of names) {
  const { av, all } = parseNames(PRESET_DEFS[name]);
  const g100 = dml.presetGuides(name, 100, 100);
  const defs = av.map((n) => g100.get(n));
  const key = `preset/${name}`;
  cap.set(key, 'av', av.join(','));
  cap.set(key, 'avd', defs.map(String).join(','));
  cap.set(key, 'guides', all.join(','));
  runPreset(key, name, av, defs, [...BUILTINS, ...all, ...PROBE_NAMES]);
}
for (const [label, name] of EXTRA_NAMES) {
  runPreset(`extra/${label}`, name, ['adj1', 'adj2'], [50000, 50000], [...BUILTINS, 'adj1', 'adj2', ...PROBE_NAMES]);
}
// the hand-drawn names that are not presets of the table get their own records
for (const name of HAND_DRAWN) {
  if (names.includes(name)) continue;
  runPreset(`hand/${name}`, name, ['adj1', 'adj2'], [50000, 50000], [...BUILTINS, 'adj1', 'adj2', ...PROBE_NAMES]);
}
{
  const s = new Stream();
  for (const n of [...names, ...HAND_DRAWN, ...EXTRA_NAMES.map((e) => e[1])]) s.bool(dml.hasPresetDef(n));
  cap.set('has/all', 'v', summary(s));
}

// ── custom guides ───────────────────────────────────────────────────────────────────────────────

const CG_DIMS = [200, 100, 0, 1e6, 21600, 13.37, NaN, -5, Infinity];
const CG_NAMES = ['a', 'b', 'c', 'd', 'x1', 'y1', '', '0', 'w', 'h', 'l', 'ss', 'wd2', 'adj1', 'adj2', 'val', '1e3', ' 12'];
const CG_OPS = ['val', '*/', '+-', '+/', '?:', 'abs', 'at2', 'cat2', 'sat2', 'cos', 'sin', 'tan', 'max', 'min', 'mod', 'pin', 'sqrt', 'bogus', '', 'VAL', '+ -'];
const CG_TOKENS = [
  '0', '1', '-1', '100000', '21600000', '5400000', '2.5', '0x1A', '1e400', '1e-400', ' 12', 'Infinity', '-Infinity', 'NaN', '+.5', '5.', '0b11',
  '0o17', '1_0', 'abc', '', 'l', 't', 'r', 'b', 'w', 'h', 'hc', 'vc', 'ss', 'ls', 'wd2', 'hd4', 'ssd8', 'cd2', 'cd4', '3cd4', 'a', 'c', 'd',
  'x1', 'y1', 'adj1', 'adj2', '\u00a0 7\u00a0', '\ufeff3', '\u0663', 'e5', '.', '-', '+', '1e3',
];
const CG_QUERY = [...new Set([...CG_TOKENS, ...CG_NAMES, ...BUILTINS, ...PROBE_TOKENS])];
const CG_PROGRAMS = 5000;
const CG_BLOCK = 250;

function program(i) {
  const rng = new Rng(0xc0de0000 + i);
  const w = CG_DIMS[rng.below(CG_DIMS.length)];
  const h = CG_DIMS[rng.below(CG_DIMS.length)];
  const nav = rng.below(4);
  const ngd = rng.below(10);
  const guide = () => {
    const name = CG_NAMES[rng.below(CG_NAMES.length)];
    const opn = rng.below(CG_OPS.length + 1);
    if (opn === CG_OPS.length) return [name, []];
    const argc = rng.below(5);
    const fmla = [CG_OPS[opn]];
    for (let k = 0; k < argc; k++) fmla.push(CG_TOKENS[rng.below(CG_TOKENS.length)]);
    return [name, fmla];
  };
  const av = [], gd = [];
  for (let k = 0; k < nav; k++) av.push(guide());
  for (let k = 0; k < ngd; k++) gd.push(guide());
  return { w, h, av, gd };
}

for (let b = 0; b < CG_PROGRAMS / CG_BLOCK; b++) {
  const s = new Stream();
  for (let i = b * CG_BLOCK; i < (b + 1) * CG_BLOCK; i++) {
    const { w, h, av, gd } = program(i);
    const val = dml.customGuides(av, gd, w, h);
    s.u32(i);
    for (const t of CG_QUERY) f(s, val(t));
  }
  cap.set(`custom/${b}`, 'v', summary(s));
}

// ── arcToSeg ────────────────────────────────────────────────────────────────────────────────────

{
  const s = new Stream();
  const centres = [0, 10.5, -3], radii = [0, -1, 5, 100, NaN, Infinity];
  const starts = [0, 5400000, 16200000, -5400000, 1e9, NaN, Infinity];
  const sweeps = [0, 5400000, -5400000, 21600000, -21600000, 30000000, 1, NaN, Infinity, -Infinity, 1e308];
  for (const cx of centres) for (const cy of centres) for (const rx of radii) for (const ry of radii) for (const st of starts) for (const sw of sweeps) {
    const r = dml.arcToSeg(cx, cy, rx, ry, st, sw);
    if (r === null) { s.tag('n'); continue; }
    s.tag('a');
    segs(s, [[r.seg]]);
    f(s, r.x); f(s, r.y);
  }
  cap.set('arc/grid', 'v', summary(s));
}

cap.write(out);
console.log(`${out}: ${cap.records.size} records`);
