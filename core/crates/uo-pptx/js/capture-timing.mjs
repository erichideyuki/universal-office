// Parity capture of timing, animation, playback and the slide show (plan section 4.3, frontier B6): what the
// frozen TS computes from the `<p:timing>` of each slide, as digests (capture) and a text sidecar of the animation
// hook calls (inputs and outputs), replayed by `tests/timing_replay.rs`.
//
//   node core/crates/uo-pptx/js/capture-timing.mjs --samples                       # the tracked .pptx samples
//   node core/crates/uo-pptx/js/capture-timing.mjs --gen b6a --decks core/crates/uo-pptx/tests/captures/decks
//   node core/crates/uo-pptx/js/capture-timing.mjs --formulas                      # the formula corpus
//   options: --out <stem> (default tests/captures/timing-<mode>)  --only <s>  --max <n>  --ts-root <dir>  --decks <dir>
//
// Per deck (key `samples/<path>` or `gen/<group>/<NNNN>`), facets (each `len:sha256[0..32]` of the UTF-8 text):
//   input   `len:sha256[0..16]` of the deck          read   `ok` | `err:<message>`          slides  count
//   t<N>    canonical JSON of `slide.timing` (parseSlideTiming) of slide N
//   tl.<v>  the playback timeline of variant v (`PB` below): slides, steps, placed effects (node as [tag, id])
//   st.<v>  playbackStateAt at sampled times + shapeOpacityAt of every target + mediaStateAt (two clocks)
//   hk      every effect of variant a evaluated through shapeAnimHooks over a fixed base (all/bg/para/unit/chart)
//   sh.<k>  the SlideShow scripts a..d: 40 random commands, the frame/due/wake after each
//   fr      the digest of the deck's section of the hook-call sidecar (`<stem>.frames.gz`)
// The sidecar lines (tab separated, numbers as the 16 hex digits of the f64 bits, strings percent-encoded) are
// documented at `frameLines` below; Rust re-runs each hook over the recorded inputs and compares the output tokens.
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { gzipSync } from 'node:zlib';
import { CaptureWriter, Rng } from '../../uo-testkit/js/testkit.mjs';
import { bytesDigest, canonJson, textDigest } from './lib/canon-json.mjs';
import { args, capturesDir, loadSlidesCore, oneLine, readSample, trackedSamples, tsCommit, tsRoot } from './lib/common.mjs';
import { GROUPS, buildGroupDeck, deckFile, groupKey } from './lib/gen/groups.mjs';
import { freshRender, recordTsSlide } from './lib/ts-paint.mjs';
import { VIEWS } from './lib/views.mjs';

const a = args();
const root = tsRoot(a);
const core = await loadSlidesCore(root);
const render = await freshRender(root);
const only = a.opt('--only');
const max = Number(a.opt('--max', '1e9'));

// ───────────────────────── encodings shared with the Rust replay ─────────────────────────

const dv = new DataView(new ArrayBuffer(8));
/** The 16 hex digits of the f64 bits (every NaN as the canonical quiet NaN). */
export function bits(x) {
  x = +x;
  if (Number.isNaN(x)) return '7ff8000000000000';
  dv.setFloat64(0, x);
  return dv.getUint32(0).toString(16).padStart(8, '0') + dv.getUint32(4).toString(16).padStart(8, '0');
}
const fixLone = (s) => s.replace(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g, '�');
/** Percent-encoding of a string (UTF-8) for a tab-separated field. */
export const esc = (s) => encodeURIComponent(fixLone(String(s))).replace(/[!'()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
const opt = (v) => (v === undefined ? '-' : `+${esc(v)}`);
const baseTokens = (b) => [bits(b.x), bits(b.y), bits(b.w), bits(b.h), b.rot === undefined ? '-' : bits(b.rot), opt(b.fill), opt(b.line), opt(b.text)];
const unitTokens = (u) => [u.para, u.k, u.count, u.kPara, u.countPara].map(String);
function stateTokens(st) {
  if (st === null || st === undefined) return ['null'];
  return ['S', st.visible ? '1' : '0', ...[st.opacity, st.x, st.y, st.w, st.h, st.rot, st.sx, st.sy, st.mx, st.my].map(bits),
    String(st.filters.length), ...st.filters.flatMap((f) => [esc(f.filter), esc(f.transition), bits(f.p)]),
    opt(st.fillColor), opt(st.lineColor), opt(st.textColor), opt(st.fontWeight), opt(st.fontStyle),
    st.underline === undefined ? '-' : st.underline ? '1' : '0', st.fontScale === undefined ? '-' : bits(st.fontScale), opt(st.fontFamily)];
}
const sha = (text) => createHash('sha256').update(text, 'utf8').digest('hex');
const digestOf = (text) => `${Buffer.byteLength(text)}:${sha(text).slice(0, 32)}`;

// ───────────────────────── playback variants ─────────────────────────

/** The playback option sets: default, the video export (33 ms tick, 30 fps transitions), no timings, a 60 Hz live tick. */
export const PB = {
  a: {},
  b: { tickMs: 33, transitionFps: 30 },
  c: { useTimings: false, defaultSlideMs: 2500 },
  d: { tickMs: 1000 / 60, defaultSlideMs: 4000 },
};
/** The show scripts: options of `SlideShow`. */
export const SH = { a: {}, b: { useTimings: false }, c: { animations: false }, d: { loop: true } };

const effRec = (e) => {
  const o = { ...e, node: [e.node.tag, e.node.id ?? null] };
  return o;
};
const tlJson = (tl) => canonJson({
  slides: tl.slides.map((s) => ({
    index: s.index, startMs: s.startMs, transition: s.transition, transitionMs: s.transitionMs, animStartMs: s.animStartMs,
    stepStartMs: s.stepStartMs, endMs: s.endMs, effects: s.effects.map(effRec),
  })),
  durationMs: tl.durationMs, tickMs: tl.tickMs, mediaClock: tl.mediaClock,
});

/** Animation times (relative to the start of the animations) worth sampling for a slide of the timeline. */
function animTimes(ps, limit) {
  const c = new Set([0, -1]);
  for (const e of ps.effects) {
    if (e.t0 >= core.STEP_PENDING) continue;
    c.add(e.t0);
    if (Number.isFinite(e.durMs)) { c.add(e.t0 + e.durMs / 2); c.add(e.t0 + e.durMs - 1); c.add(e.t0 + e.durMs + 10); }
    else c.add(e.t0 + 700);
  }
  const all = [...c].filter((x) => Number.isFinite(x)).sort((x, y) => x - y);
  if (all.length <= limit) return all;
  const out = [];
  for (let i = 0; i < limit; i++) out.push(all[Math.round((i * (all.length - 1)) / (limit - 1))]);
  return [...new Set(out)];
}

/** Times (ms of the timeline) sampled for the state facet: around every slide boundary and animation. */
function stateTimes(tl) {
  const ts = [-5, 0, tl.durationMs, tl.durationMs + 1000, tl.durationMs / 2];
  for (const s of tl.slides) {
    ts.push(s.startMs, s.startMs + s.transitionMs / 2, s.animStartMs - 1, s.animStartMs, s.endMs - 1, s.endMs);
    for (const ms of animTimes(s, 4)) ts.push(s.animStartMs + ms);
  }
  const u = [...new Set(ts.filter((x) => Number.isFinite(x)))].sort((x, y) => x - y);
  return u.length > 80 ? u.filter((_, i) => i % Math.ceil(u.length / 80) === 0) : u;
}

function statesJson(tl, models) {
  const out = [];
  const clocks = [{}, { latencyMs: 66, scale: 0.9 }];
  for (const t of stateTimes(tl)) {
    const st = core.playbackStateAt(tl, t);
    const rec = { t, pos: st.pos, from: st.from ? tl.slides.indexOf(st.from) : null, tp: st.transitionProgress, anim: st.animMs, ended: st.ended };
    if (st.slide) {
      const spids = [...new Set(st.slide.effects.map((e) => e.target?.spid).filter(Boolean))];
      rec.op = {};
      for (const sp of spids) rec.op[sp] = core.shapeOpacityAt(st.slide.effects, [sp], st.animMs);
      const m = models[st.slide.index];
      rec.media = [];
      for (const mi of m?.media ?? []) {
        rec.media.push(clocks.map((ck) => core.mediaStateAt(st.slide.effects, mi.spid, st.animMs, ck, !!mi.loop)));
      }
      rec.cmds = (m?.media ?? []).map((mi) => core.mediaCommands(st.slide.effects, mi.spid));
    }
    out.push(rec);
  }
  return canonJson(out);
}

// ───────────────────────── direct hook sampling ─────────────────────────

const HK_BASE = { x: 0.5, y: 0.4, w: 0.2, h: 0.1, rot: 15, fill: '#336699', line: '#CC0000', text: '#00AA00' };
const HK_UNITS = [
  [{ para: 0, k: 0, count: 6, kPara: 0, countPara: 3 }, undefined],
  [{ para: 0, k: 2, count: 6, kPara: 2, countPara: 3 }, { para: 0, k: 1, count: 4, kPara: 1, countPara: 2 }],
  [{ para: 1, k: 3, count: 6, kPara: 0, countPara: 3 }, { para: 1, k: 2, count: 4, kPara: 0, countPara: 2 }],
  [{ para: 2, k: 5, count: 6, kPara: 2, countPara: 3 }, undefined],
];

/** Every target of the effects of `ps` through shapeAnimHooks at sampled times: the text of all the results. */
function hookSweep(ps, model) {
  const bg = new Set(model?.animBg ?? []);
  const actx = { animBg: (spid) => bg.has(spid) };
  const lines = [];
  const spids = [...new Set(ps.effects.map((e) => e.target?.spid).filter(Boolean))];
  const dgmIds = [undefined, ...new Set(ps.effects.map((e) => e.target?.dgm).filter(Boolean))];
  for (const ms of animTimes(ps, 6)) {
    for (const sp of spids) {
      for (const dgm of dgmIds) {
        const h = core.shapeAnimHooks(ps.effects, [sp, 'g1'], ms, dgm ? { ...actx, dgmId: dgm } : actx);
        lines.push(`H\t${esc(sp)}\t${bits(ms)}\t${dgm ? esc(dgm) : '-'}\t${h ? hookFlags(h) : 'null'}`);
        if (!h) continue;
        lines.push(['A', 'all', ...baseTokens(HK_BASE), '=', ...stateTokens(h.all(HK_BASE))].join('\t'));
        if (h.bg) lines.push(['A', 'bg', ...baseTokens(HK_BASE), '=', ...stateTokens(h.bg(HK_BASE))].join('\t'));
        if (h.para) for (let i = 0; i < 3; i++) lines.push(['A', 'para', String(i), ...baseTokens(HK_BASE), '=', ...stateTokens(h.para(i, HK_BASE))].join('\t'));
        if (h.unit) for (const [u, w] of HK_UNITS) lines.push(['A', 'unit', ...unitTokens(u), ...baseTokens(HK_BASE), w ? unitTokens(w).join(',') : '-', '=', ...stateTokens(h.unit.state(u, HK_BASE, w))].join('\t'));
        lines.push(...chartLines(h));
      }
    }
  }
  return lines;
}

function hookFlags(h) {
  const f = [];
  if (h.bg) f.push('bg:1');
  if (h.para) f.push('para:1');
  if (h.unit) f.push(`unit:${h.unit.type}`, `words:${h.unit.words ? 1 : 0}`);
  if (h.chartParts) f.push(`chart:${h.chartParts.length}`);
  return f.join(',') || 'plain';
}

/** `C` lines (the structure of each chart part) and the `A chart` lines over a fixed base. */
function chartLines(h) {
  const out = [];
  (h.chartParts ?? []).forEach((p, j) => {
    let probe = '';
    if (p.sel.data === null) probe = '-';
    else for (const s of [-1, 0, 1, 2]) for (const k of [-1, 0, 1, 2]) probe += p.sel.data(s, k) ? '1' : '0';
    out.push(['C', String(j), p.sel.key === undefined ? '-' : esc(p.sel.key), p.sel.bg ? '1' : '0', p.state ? '1' : '0', probe].join('\t'));
    if (p.state) out.push(['A', 'chart', String(j), ...baseTokens(HK_BASE), '=', ...stateTokens(p.state(HK_BASE))].join('\t'));
  });
  return out;
}

// ───────────────────────── painted frames (the recorded hook calls) ─────────────────────────

/**
 * Paints `ps` at `animMs` with the frozen TS painter over the recording context and records every anim hook call
 * (`shapeAnim` wrapped: the base in, the PartState out) and the media `shapeOpacity`, as the viewer wires them.
 *
 * Lines: `H shapeIds dgmId flags` (flags `null` when the hook set is null) · `C j key bg animated probe` per chart
 * part (+ `A chart …` over a fixed base) · `A all|bg|para|unit …` per call: `A kind [args] base… [word] = state…` ·
 * `O op` the shape opacity of a media shape. Frames: `F tl v t` / `F sh k step now` then `P index animMs`.
 */
async function paintFrame(slide, pres, ps, animMs, model, view, out) {
  const bg = new Set(model?.animBg ?? []);
  const actx = { animBg: (spid) => bg.has(spid) };
  const mediaInfo = new Map((model?.media ?? []).map((m) => [m.spid, m]));
  const m = { ox: view.ox, oy: view.oy, sw: pres.slideW * view.scale, sh: pres.slideH * view.scale };
  const wrap = (sh) => {
    if (sh.inherited || !ps.effects.length) return null;
    const ids = sh.groupIds ? [sh.id, ...sh.groupIds] : [sh.id];
    const h = core.shapeAnimHooks(ps.effects, ids, animMs, sh.dgmId ? { ...actx, dgmId: sh.dgmId } : actx);
    out.push(`H\t${ids.map(esc).join(',')}\t${sh.dgmId ? esc(sh.dgmId) : '-'}\t${h ? hookFlags(h) : 'null'}`);
    if (!h) return null;
    const w = { all: (b) => { const s = h.all(b); out.push(['A', 'all', ...baseTokens(b), '=', ...stateTokens(s)].join('\t')); return s; } };
    if (h.bg) w.bg = (b) => { const s = h.bg(b); out.push(['A', 'bg', ...baseTokens(b), '=', ...stateTokens(s)].join('\t')); return s; };
    if (h.para) w.para = (i, b) => { const s = h.para(i, b); out.push(['A', 'para', String(i), ...baseTokens(b), '=', ...stateTokens(s)].join('\t')); return s; };
    if (h.unit) {
      w.unit = {
        ...h.unit,
        state: (u, b, wd) => { const s = h.unit.state(u, b, wd); out.push(['A', 'unit', ...unitTokens(u), ...baseTokens(b), wd ? unitTokens(wd).join(',') : '-', '=', ...stateTokens(s)].join('\t')); return s; },
      };
    }
    if (h.chartParts) { out.push(...chartLines(h)); w.chartParts = h.chartParts; }
    void m;
    return w;
  };
  const opts = {
    ox: view.ox, oy: view.oy,
    shapeAnim: wrap,
    shapeOpacity: (sh) => {
      const mi = sh.media ? mediaInfo.get(sh.id) : undefined;
      const op = mi?.showWhenStopped === false && !core.mediaStateAt(ps.effects, sh.id, animMs, {}, !!mi.loop).playing ? 0 : 1;
      if (sh.media) out.push(`O\t${esc(sh.id)}\t${bits(op)}`);
      return op;
    },
  };
  const r = await recordTsSlide({ tsRoot: root, slide, pres, scale: view.scale, ox: view.ox, oy: view.oy, opts, render });
  if (r.error) out.push(`E\t${esc(oneLine(r.error))}`);
}

// ───────────────────────── the show scripts ─────────────────────────

const fnv = (s) => { let h = 0x811c9dc5; for (const c of Buffer.from(s, 'utf8')) { h ^= c; h = Math.imul(h, 0x01000193) >>> 0; } return h >>> 0; };
const DT = [0, 0, 16, 100, 700, 1500, 4000, 12000];

/** One show script: 40 commands; returns the JSON text of the results and the checkpoints worth painting. */
function runShow(key, k, models) {
  const rng = new Rng(fnv(`${key}#${k}`));
  const n = models.length;
  let now = 1000;
  const from = rng.below(n + 1);
  const ss = new core.SlideShow(models, from, now, SH[k]);
  const rec = [];
  const checks = [];
  for (let i = 0; i < 40; i++) {
    const kind = rng.below(100);
    now += DT[rng.below(DT.length)];
    let name, ret;
    if (kind < 40) { name = 'next'; const mouse = rng.below(2) === 1; ret = ss.next(now, mouse); name += mouse ? 'm' : ''; }
    else if (kind < 55) { name = 'prev'; ret = ss.prev(now); }
    else if (kind < 62) { const g = rng.below(n + 2) - 1; name = `go${g}`; ret = ss.goTo(g, now); }
    else if (kind < 66) { name = 'first'; ret = ss.first(now); }
    else if (kind < 70) { name = 'last'; ret = ss.last(now); }
    else { name = 'upd'; ret = 'none'; }
    const u = ss.update(now);
    const f = ss.frame(now);
    rec.push({
      i, name, now, ret, u, idx: ss.index, step: ss.step, steps: ss.steps, ended: ss.ended, due: ss.dueAt(), wake: ss.wakeAt(now),
      f: {
        slide: f.slide ? f.slide.index : null, animMs: f.animMs, tp: f.transitionProgress, ended: f.ended,
        from: f.from ? { index: f.from.slide.index, animMs: f.from.animMs, key: f.from.key } : null,
        ps: f.slide ? {
          startMs: f.slide.startMs, transition: f.slide.transition, transitionMs: f.slide.transitionMs, animStartMs: f.slide.animStartMs,
          stepStartMs: f.slide.stepStartMs, effects: f.slide.effects.map((e) => [e.t0, e.step, e.endAt ?? null, e.durMs, e.startMs]),
        } : null,
      },
    });
    if (f.slide && f.slide.effects.some((e) => e.t0 < core.STEP_PENDING)) checks.push({ i, now, ps: f.slide, animMs: f.animMs });
  }
  return { text: canonJson(rec), checks };
}

// ───────────────────────── one deck ─────────────────────────

const writer = new CaptureWriter(['Slides timing/playback/show (B6): digests of the TS timing model, timelines, states, hook sweeps and show scripts; see js/capture-timing.mjs and core/docs/modules/uo-pptx-captures.md']);
writer.meta('ts-root-commit', tsCommit(root));
writer.meta('variants', `playback ${JSON.stringify(PB)}; show ${JSON.stringify(SH)}`);
const frames = [];
let nDecks = 0, nSlides = 0, nFrames = 0, nCalls = 0, nErr = 0;
const t0 = Date.now();

async function record(key, bytes, extra = {}) {
  if (only && !key.includes(only)) return;
  if (nDecks >= max) return;
  nDecks++;
  writer.set(key, 'input', bytesDigest(bytes));
  for (const [k, v] of Object.entries(extra)) writer.set(key, k, v);
  let pkg, pres;
  try { pkg = core.PptxPackage.load(bytes); pres = core.readPresentation(pkg); } catch (e) { writer.set(key, 'read', `err:${oneLine(e?.message ?? e)}`); return; }
  writer.set(key, 'read', 'ok');
  writer.set(key, 'slides', pres.slides.length);
  const slides = [];
  pres.slides.forEach((ref, i) => {
    try {
      const s = ref.target === undefined ? undefined : core.readSlide(pkg, ref.target);
      slides.push(s);
      writer.set(key, `t${i}`, s ? textDigest(canonJson(s.timing)) : 'err:no-target');
      nSlides++;
    } catch (e) { slides.push(undefined); writer.set(key, `t${i}`, `err:${oneLine(e?.message ?? e)}`); }
  });
  const models = slides.map((s) => s?.timing);
  const section = [`#deck\t${key}`];
  let tla;
  for (const [v, opts] of Object.entries(PB)) {
    const tl = core.buildPlaybackTimeline(models, opts);
    if (v === 'a') tla = tl;
    writer.set(key, `tl.${v}`, textDigest(tlJson(tl)));
    writer.set(key, `st.${v}`, textDigest(statesJson(tl, models)));
  }
  // direct hook sweep over every slide of the default timeline
  const sweep = [];
  for (const ps of tla.slides) if (ps.effects.length) sweep.push(`#slide\t${ps.index}`, ...hookSweep(ps, models[ps.index]));
  writer.set(key, 'hk', textDigest(sweep.join('\n')));
  // show scripts
  const checks = [];
  for (const k of Object.keys(SH)) {
    const r = runShow(key, k, models);
    writer.set(key, `sh.${k}`, textDigest(r.text));
    if (k === 'a') checks.push(...r.checks);
  }
  // painted frames (the recorded hook calls): timeline variants a and b, and the show script a
  const view = VIEWS[nDecks % 2 === 0 ? 'a' : 'b'](pres);
  for (const v of ['a', 'b']) {
    const tl = v === 'a' ? tla : core.buildPlaybackTimeline(models, PB.b);
    for (const ps of tl.slides) {
      if (!ps.effects.length || !slides[ps.index]) continue;
      const ts = animTimes(ps, v === 'a' ? 5 : 3).map((ms) => ps.animStartMs + ms);
      if (tl.slides[tl.slides.length - 1] === ps) ts.push(tl.durationMs + 1);
      for (const t of ts) {
        const st = core.playbackStateAt(tl, t);
        if (!st.slide || !st.slide.effects.length || !slides[st.slide.index]) continue;
        const out = [`F\ttl\t${v}\t${bits(t)}`, `P\t${st.slide.index}\t${bits(st.animMs)}`];
        await paintFrame(slides[st.slide.index], pres, st.slide, st.animMs, models[st.slide.index], view, out);
        section.push(...out);
        nFrames++;
      }
    }
  }
  for (const c of checks.slice(0, 6)) {
    if (!slides[c.ps.index]) continue;
    const out = [`F\tsh\ta\t${c.i}\t${bits(c.now)}`, `P\t${c.ps.index}\t${bits(c.animMs)}`];
    await paintFrame(slides[c.ps.index], pres, c.ps, c.animMs, models[c.ps.index], view, out);
    section.push(...out);
    nFrames++;
  }
  nCalls += section.filter((l) => l.startsWith('A\t')).length;
  nErr += section.filter((l) => l.startsWith('E\t')).length;
  const text = section.join('\n') + '\n';
  writer.set(key, 'fr', textDigest(text));
  frames.push(text);
}

// ───────────────────────── formulas ─────────────────────────

const FORMULA_VARS = [
  { '#ppt_x': 0.5, '#ppt_y': 0.4, '#ppt_w': 0.2, '#ppt_h': 0.1, '#ppt_r': 15, ppt_x: 0.5, ppt_y: 0.4, ppt_w: 0.2, ppt_h: 0.1, ppt_r: 15 },
  { '#ppt_x': 0, '#ppt_y': 0, '#ppt_w': 0, '#ppt_h': 0, '#ppt_r': 0, ppt_x: 0, ppt_y: 0, ppt_w: 0, ppt_h: 0, ppt_r: 0 },
  { '#ppt_x': 1.25, '#ppt_y': -0.3, '#ppt_w': 3, '#ppt_h': 0.007, '#ppt_r': -90, ppt_x: 1.25, ppt_y: -0.3, ppt_w: 3, ppt_h: 0.007, ppt_r: -90, $: 0.37 },
];

function formulaCorpus(rng) {
  const out = new Set(['', ' ', '1', '-1', '.5', '1.', '1e3', '1E-2', '1e+2', '-.5e-1', '0.5.5', '#ppt_x', '#PPT_X', 'ppt_w', '$', '$x', 'pi', 'PI', 'e', 'E', '2e', '2e-', 'hidden', 'bold', 'visible',
    '1+', '+', '-', '()', '(1', '1)', '2*)', '#ppt_x + *', '1,2', 'max(1,2,3)', 'min()', 'max()', 'rand(4)', 'rand()', 'sqrt(-4)', 'ln(0)', 'ln(2)', 'exp(1)', '1/0', '0/0', '2^3^2', '-2^2', '2^-1', '2^0.5',
    '(#ppt_y+0.4)', '1+#ppt_h/2', '0-#ppt_w/2', '#ppt_x+0.4*cos(pi*(1-$))', '$*$', 'sin(pi/2)', 'cos(0)', 'tan(pi/4)', 'asin(2)', 'acos(0.5)', 'atan(1)', 'sinh(1)', 'cosh(1)', 'tanh(1)', 'deg(pi)', 'rad(180)',
    'abs(-3)', 'floor(2.7)', 'ceil(2.1)', 'floor(-2.5)', 'foo(1)', 'foo', 'constructor', 'a b', '1 2', '１', '1+２', '😀', '1+😀', '  #ppt_x  ', '\t1+1\n', '1e999', '1e-999', '9'.repeat(400), '((((((((((1))))))))))',
    '('.repeat(150) + '1' + ')'.repeat(150), '1' + '+1'.repeat(300), '-'.repeat(200) + '1', 'max(' + '1,'.repeat(200) + '1)', 'sin('.repeat(130) + '1' + ')'.repeat(130)]);
  const atoms = ['1', '2', '0.5', '3.25', '0', '#ppt_x', '#ppt_y', '#ppt_w', '#ppt_h', '#ppt_r', 'ppt_w', '$', 'pi', 'e', 'x', '1e2'];
  const ops = ['+', '-', '*', '/', '^'];
  const fns = ['sin', 'cos', 'tan', 'asin', 'acos', 'atan', 'sinh', 'cosh', 'tanh', 'deg', 'rad', 'sqrt', 'abs', 'ln', 'exp', 'max', 'min', 'floor', 'ceil', 'rand', 'nope'];
  const gen = (d) => {
    const k = rng.below(10);
    if (d > 4 || k < 3) return atoms[rng.below(atoms.length)];
    if (k < 5) return `${gen(d + 1)}${ops[rng.below(ops.length)]}${gen(d + 1)}`;
    if (k < 6) return `-${gen(d + 1)}`;
    if (k < 7) return `(${gen(d + 1)})`;
    const f = fns[rng.below(fns.length)];
    const n = rng.below(4);
    return `${f}(${Array.from({ length: n }, () => gen(d + 1)).join(',')})`;
  };
  const junk = ['(', ')', ',', '+', '*', '#', '$', '.', 'e', ' ', '1', 'x', '^', '-', '/'];
  while (out.size < 1500) {
    if (rng.below(10) < 8) out.add(gen(0));
    else { let s = ''; for (let i = rng.below(8) + 1; i > 0; i--) s += junk[rng.below(junk.length)]; out.add(s); }
  }
  return [...out];
}

function formulaSection(extra) {
  const rng = new Rng(0x5eed1234);
  const list = [...formulaCorpus(rng), ...extra].filter((s, i, arr) => arr.indexOf(s) === i);
  const lines = list.map((f) => [esc(f), ...FORMULA_VARS.map((v) => bits(core.evalFormula(f, v)))].join('\t'));
  return lines;
}

/** The pure numeric helpers: accelDecel, gifFrameAt, mediaFadeAlpha over fixed grids (one digest each). */
function fnFacets(w, key) {
  const f = [0, 0.1, 0.25, 0.5, 0.75, 0.9, 1, -0.1, 1.2, NaN];
  const ad = [0, 0.1, 0.3, 0.5, 0.9, 1, NaN, -0.2];
  const acc = [];
  for (const x of f) for (const p of ad) for (const q of ad) acc.push(bits(core.accelDecel(x, p, q)));
  w.set(key, 'acc', digestOf(acc.join(',')));
  const delays = [[], [100], [100, 50], [5, 5, 5], [100, 50, 5, 200], [0, 0], [10, 11, 1000]];
  const el = [0, -1, 1, 99, 100, 150, 449, 450, 451, 900, 100000, NaN, Infinity];
  const rp = [0, 1, 3, Infinity];
  const gif = [];
  for (const d of delays) for (const e of el) for (const r of rp) gif.push(core.gifFrameAt(d, e, r));
  w.set(key, 'gif', digestOf(gif.join(',')));
  const fades = [undefined, { inMs: 0, outMs: 0 }, { inMs: 500, outMs: 0 }, { inMs: 0, outMs: 500 }, { inMs: 500, outMs: 500 }];
  const fa = [];
  for (const fd of fades) for (const p of [-10, 0, 100, 250, 500, 1000, 4000, NaN]) for (const l of [undefined, 1000, 3000, Infinity, NaN]) fa.push(bits(core.mediaFadeAlpha(fd, p, l)));
  w.set(key, 'fade', digestOf(fa.join(',')));
}

// ───────────────────────── driver ─────────────────────────

let mode;
let sidecar = null;
if (a.flag('--formulas')) {
  mode = 'formula';
  // formulas the decks carry (tav fmla, strVal, from/to/by) join the generated corpus
  const extra = new Set();
  const decksDir = a.opt('--decks');
  const collect = (bytes) => {
    try {
      const pkg = core.PptxPackage.load(bytes);
      const pres = core.readPresentation(pkg);
      for (const ref of pres.slides) {
        const s = core.readSlide(pkg, ref.target);
        const walk = (n) => {
          for (const k of n.tav ?? []) { if (typeof k.val === 'string') extra.add(k.val); if (k.fmla) extra.add(k.fmla); }
          for (const v of [n.from, n.to, n.by, n.setTo]) if (typeof v === 'string') extra.add(v);
          n.children.forEach(walk);
        };
        for (const st of s.timing.steps) for (const e of st.effects) walk(e.node);
        for (const e of s.timing.interactive) walk(e.node);
      }
    } catch { /* unreadable deck */ }
  };
  for (const rel of trackedSamples()) if (/\.pptx$/i.test(rel)) collect(readSample(rel));
  if (decksDir) for (let i = 0; i < GROUPS.b6a.count; i++) collect(new Uint8Array(readFileSync(join(decksDir, 'b6a', deckFile('b6a', i)))));
  const lines = formulaSection([...extra].sort());
  sidecar = { name: 'formulas', text: lines.join('\n') + '\n' };
  writer.set('formula/ts', 'count', lines.length);
  writer.set('formula/ts', 'results', digestOf(sidecar.text));
  fnFacets(writer, 'fn/media');
} else if (a.flag('--samples')) {
  mode = 'samples';
  for (const rel of trackedSamples()) if (/\.pptx$/i.test(rel)) await record(`samples/${rel}`, readSample(rel));
} else if (a.opt('--gen')) {
  const names = a.opt('--gen').split(',');
  mode = names.join('+');
  const decksDir = a.opt('--decks');
  for (const name of names) {
    const g = GROUPS[name];
    if (!g) throw new Error(`unknown group ${name}`);
    for (let i = 0; i < g.count; i++) {
      const d = decksDir ? null : buildGroupDeck(name, i);
      const bytes = decksDir ? new Uint8Array(readFileSync(join(decksDir, name, deckFile(name, i)))) : new Uint8Array(d.bytes);
      await record(groupKey(name, i), bytes);
    }
  }
} else {
  console.error('usage: capture-timing.mjs (--samples | --gen <group[,group]> | --formulas) [--out stem] [--only s] [--max n] [--decks dir]');
  process.exit(2);
}

const stem = a.opt('--out', join(capturesDir, `timing-${mode}`));
mkdirSync(dirname(stem), { recursive: true });
writer.write(`${stem}.capture`);
let side = 0;
if (sidecar) { const gz = gzipSync(Buffer.from(sidecar.text, 'utf8'), { level: 9 }); writeFileSync(`${stem}.${sidecar.name}.gz`, gz); side = gz.length; }
else { const gz = gzipSync(Buffer.from(frames.join(''), 'utf8'), { level: 9 }); writeFileSync(`${stem}.frames.gz`, gz); side = gz.length; }
console.log(`${mode}: ${nDecks} decks, ${nSlides} slides, ${nFrames} painted frames, ${nCalls} hook calls, ${nErr} paint errors, sidecar ${side} B, ${((Date.now() - t0) / 1000).toFixed(1)} s -> ${stem}.*`);
