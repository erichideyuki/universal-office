// Parity capture of D2b-F7's pure model file (record core/docs/modules/uo-docx-d2b/F7.md): the frozen
// editor's `picture-ops.ts` (effectExtentOf, zFor, fillFitCrop, aspectCrop and the effect presets the
// galleries apply by id), over seeded inputs.
//
//   UO_TS_ROOT=<built checkout of main> node --experimental-transform-types --no-warnings \
//     core/crates/uo-docx/js/capture-d2b-f7-ops.mjs [--cases N]
//
// Writes tests/parity/d2b-f7-ops.capture. Keys `ee<j>`, `z<j>`, `ff<j>`, `ac<j>` (facets `in`, `out`)
// and `pre#<kind>:<id>` (facet `out`). `effectExtentOf` calls Math.sin/cos/tan: Node 24.15 here, the
// uo-jsnum port on the Rust side (proven equal in the uo-jsnum record).
import { join } from 'node:path';
import { CaptureWriter, Rng, importFrom } from '../../uo-testkit/js/testkit.mjs';
import { crate, root, ts } from './capture-r2-common.mjs';

const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const CASES = opt('--cases', 3000);
const po = await importFrom(root, 'write/packages/editor/src/picture-ops.ts');
const so = await importFrom(root, 'write/packages/editor/src/shape-ops.ts');
const sx = await ts('shape-xml.js');

const out = new CaptureWriter(['D2b-F7 pure file: picture-ops.ts', 'facets: in, out']);
out.meta('cases', CASES);
const rng = new Rng(0xf7f7);
const pick = (a) => a[rng.below(a.length)];
const ALGN = ['tl', 't', 'tr', 'l', 'ctr', 'r', 'bl', 'b', 'br', 'x'];
const num = () => pick([0, 1, 7, 100, 9525, 12700, 38100, 50800, 63500, 91440, 914400, 2540000, 5400000, 2700000, 13500000, 21600000, 22500, 30000, 60000 * 30, 120000, 1234567]) + (rng.below(3) === 0 ? rng.below(1000) : 0);
const f = (x) => (x === undefined ? '-' : String(x));
for (let j = 0; j < CASES; j++) {
  // effectExtentOf: cx cy rot | line (-, null, fill, raw) w | glow | shadow | reflection
  const cx = num() + 1, cy = num() + 1, rot = pick([0, 0, 0, 60000 * 30, 60000 * 90, 60000 * 45, 5400000, 1234567, 21599999]);
  const lk = rng.below(5);
  const lw = rng.below(2) ? num() : undefined;
  const line = lk === 0 ? undefined : lk === 1 ? null : lk === 2 ? { w: lw, fill: { kind: 'srgbClr', val: '000000' } } : lk === 3 ? { w: lw, fillRaw: '<a:x/>' } : { w: lw, fill: null };
  const glow = rng.below(4) === 0 ? num() : undefined;
  const sk = rng.below(4);
  const sh = sk === 0 ? undefined : { kind: sk === 3 ? 'inner' : 'outer', blurRad: rng.below(2) ? num() : undefined, dist: rng.below(2) ? num() : undefined, dir: rng.below(2) ? num() : undefined, sx: rng.below(3) === 0 ? pick([102000, 90000, 50000]) : undefined, sy: rng.below(3) === 0 ? pick([23000, -19000, 90000]) : undefined, kx: rng.below(3) === 0 ? pick([1200000, -1200000, 800400]) : undefined, ky: rng.below(4) === 0 ? pick([600000, -600000]) : undefined, algn: rng.below(2) ? pick(ALGN) : undefined, color: { kind: 'prstClr', val: 'black' } };
  const rk = rng.below(4);
  const rf = rk === 0 ? { blurRad: rng.below(2) ? 6350 : undefined, dist: rng.below(2) ? 50800 : undefined, endPos: rng.below(2) ? pick([35000, 90000]) : undefined } : undefined;
  const fm = { cx, cy, rot, line, effects: { shadow: sh, glow: glow === undefined ? undefined : { rad: glow, color: { kind: 'srgbClr', val: '000000' } }, reflection: rf } };
  const lineS = lk === 0 ? '-' : lk === 1 ? 'N' : lk === 2 ? `F${f(lw)}` : lk === 3 ? `R${f(lw)}` : `Z${f(lw)}`;
  const shS = sh ? [sh.kind, f(sh.blurRad), f(sh.dist), f(sh.dir), f(sh.sx), f(sh.sy), f(sh.kx), f(sh.ky), f(sh.algn)].join(',') : '-';
  const rfS = rf ? [f(rf.blurRad), f(rf.dist), f(rf.endPos)].join(',') : '-';
  out.set(`ee${j}`, 'in', [cx, cy, rot, lineS, f(glow), shS, rfS].join('|'));
  const e = po.effectExtentOf(fm);
  out.set(`ee${j}`, 'out', `${e.l},${e.t},${e.r},${e.b}`);
  // zFor
  const others = Array.from({ length: rng.below(6) }, () => 251658240 + rng.below(20) * 1024 + (rng.below(3) === 0 ? rng.below(1024) : 0));
  const mine = rng.below(3) === 0 ? pick(others.length ? others : [251658240]) : 251658240 + rng.below(20) * 1024;
  const op = pick(['front', 'back', 'forward', 'backward']);
  out.set(`z${j}`, 'in', `${op}|${mine}|${others.join(',')}`);
  out.set(`z${j}`, 'out', String(po.zFor(op, mine, others)));
  // fillFitCrop / aspectCrop
  const bw = 1 + rng.below(9000000), bh = 1 + rng.below(9000000), nw = 1 + rng.below(2000), nh = 1 + rng.below(2000);
  const mode = pick(['fill', 'fit']);
  out.set(`ff${j}`, 'in', `${mode}|${bw}|${bh}|${nw}|${nh}`);
  const r = po.fillFitCrop(mode, { cx: bw, cy: bh }, { w: nw, h: nh });
  out.set(`ff${j}`, 'out', `${r.l},${r.t},${r.r},${r.b}`);
  const sr = { l: rng.below(30000) - 5000, t: rng.below(30000) - 5000, r: rng.below(30000) - 5000, b: rng.below(30000) - 5000 };
  const ratio = pick([1, 4 / 3, 16 / 9, 3 / 2, 3 / 4, 2 / 3, 9 / 16, 5 / 4]);
  out.set(`ac${j}`, 'in', `${bw}|${bh}|${sr.l},${sr.t},${sr.r},${sr.b}|${ratio}`);
  const a = po.aspectCrop({ cx: bw, cy: bh, srcRect: sr }, ratio);
  out.set(`ac${j}`, 'out', `${a.crop.l},${a.crop.t},${a.crop.r},${a.crop.b},${a.cx},${a.cy}`);
}
const col = (c) => (c ? `${c.kind}:${c.val}:${(c.mods ?? []).map((m) => `${m[0]}=${m[1]}`).join(';')}` : '-');
for (const p of po.SHADOW_PRESETS) { const s = p.shadow; out.set(`pre#shadow:${p.id}`, 'out', [s.kind, f(s.blurRad), f(s.dist), f(s.dir), f(s.sx), f(s.sy), f(s.kx), f(s.ky), f(s.algn), col(s.color)].join(',')); }
for (const p of po.REFLECTION_PRESETS) { const s = p.reflection; out.set(`pre#reflection:${p.id}`, 'out', [s.blurRad, s.stA, s.endA, s.endPos, f(s.dist), s.dir, s.sy, s.algn, s.rotWithShape].join(',')); }
for (const p of po.RECOLOR_PRESETS) { const x = p.fx; out.set(`pre#recolor:${p.id}`, 'out', [f(x.grayscl), f(x.biLevel), x.lum ? `${x.lum.bright}/${x.lum.contrast}` : '-', x.duotone ? `${col(x.duotone[0])}>${col(x.duotone[1])}` : '-'].join(',')); }
for (const prst of ['circle', 'relaxedInset', 'hardEdge']) { const b = po.bevel3D(prst); out.set(`pre#bevel:${prst}`, 'out', `${b.camera.prst},${b.lightRig.rig},${b.lightRig.dir},${b.bevelT.prst ?? '-'}`); }
for (let i = 0; i < 4; i++) for (let a = 1; a <= 6; a++) { const g = po.glowPreset(po.GLOW_SIZES[i], a); out.set(`pre#glow:${i}-${a}`, 'out', `${g.rad},${col(g.color)}`); }
// the gallery boxes: `textboxTemplateSpec(id, { accent, lang })` through `newTextboxDrawingXml` (the
// call `newTextboxXml` makes), every id x accent x language, and the unknown ids (`null`)
for (const id of [...so.TEXTBOX_TEMPLATES.map((t) => t.id), 'nope', '', 'user:x', 'Simple'])
  for (const accent of ['156082', '4472C4', 'ED7D31', '000000'])
    for (const lang of ['en', 'pt', 'es']) {
      const t = so.textboxTemplateSpec(id, { accent, lang });
      const xml = t ? sx.newTextboxDrawingXml({ id: 1, name: 'x', cx: t.cx, cy: t.cy, posH: t.posH, posV: t.posV, relativeHeight: 1, wrap: t.wrap, ...(t.sizeRelH ? { sizeRelH: t.sizeRelH } : {}), ...(t.sizeRelV ? { sizeRelV: t.sizeRelV } : {}), fill: t.fill, line: t.line, body: { ...t.body }, content: t.content }) : '-';
      out.set(`tt#${id}|${accent}|${lang}`, 'out', xml);
    }
out.write(join(crate, 'tests', 'parity', 'd2b-f7-ops.capture'));
console.log(`d2b-f7-ops: ${CASES} cases`);
