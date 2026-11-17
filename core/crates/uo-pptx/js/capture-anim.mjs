// Draw-op streams of ANIMATED frames of the frozen TS painter (frontier B4b): the slides of the default playback timeline
// painted at sampled times by `paintSlideTo` with the hooks the viewer's `paintPlaybackSlide` wires (`shapeAnim`,
// `shapeOpacity`, `mediaFrame`) over the recording context. The Rust side (`uo-wasm/tests/slides_anim.test.mjs`) builds the
// same timeline in uo-slides.wasm, emits the stage list with slots and the frame's `SlotFrame`, paints them with
// `paint-slides.ts` and compares the draw form line by line.
//
//   node core/crates/uo-pptx/js/capture-anim.mjs --gen b6a --decks core/crates/uo-pptx/tests/captures/decks
//   node core/crates/uo-pptx/js/capture-anim.mjs --samples
//   options: --out <stem> (default tests/captures/anim-<mode>)  --only <s>  --max <n>  --per <frames per slide, 6>  --ts-root <dir>
//
// Stream file: `#deck <key>` then per frame `#frame <slide> t=<t> anim=<animMs> view=<v> scale=<s> ox=<ox> oy=<oy> canvas=<w>x<h>
// media=<0|1> lines=<n> sha=<h>` followed by the draw-form lines; `#error <message>` after the header of a frame that threw.
// The media frame is a deterministic stub (`mediaStub` below, the same on the Rust side): shape index i at frame number f
// shows nothing, an opaque frame or a half-faded frame by `(i + f) % 3`.
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { gzipSync } from 'node:zlib';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { args, capturesDir, loadSlidesCore, oneLine, readSample, trackedSamples, tsCommit, tsRoot } from './lib/common.mjs';
import { GROUPS, buildGroupDeck, deckFile, groupKey } from './lib/gen/groups.mjs';
import { readFileSync, existsSync } from 'node:fs';
import { recordTsSlide } from './lib/ts-paint.mjs';
import { digest } from './lib/opstream.mjs';
import { VIEWS } from './lib/views.mjs';

const a = args();
const root = tsRoot(a);
const core = await loadSlidesCore(root);
const only = a.opt('--only');
const max = Number(a.opt('--max', '1e9'));
const per = Number(a.opt('--per', '6'));

/** The media frame stub: `null`, an opaque 64 x 48 frame, or the frame at alpha 0.5, by `(shape + frame) % 3`. */
export function mediaStub(shape, frame) {
  const k = (shape + frame) % 3;
  if (k === 0) return null;
  const image = { width: 64, height: 48, naturalWidth: 64, naturalHeight: 48, __h: 'media-frame-stub' };
  return { image, alpha: k === 1 ? 1 : 0.5 };
}

/** Animation times (relative to the start of the animations) worth sampling: before, starts, middles, ends of the effects. */
function animTimes(ps, limit) {
  const c = new Set([-1, 0]);
  for (const e of ps.effects) {
    if (e.t0 >= core.STEP_PENDING) continue;
    c.add(e.t0);
    if (Number.isFinite(e.durMs)) { c.add(e.t0 + e.durMs * 0.37); c.add(e.t0 + e.durMs / 2); c.add(e.t0 + e.durMs + 10); }
    else c.add(e.t0 + 700);
  }
  const all = [...c].filter((x) => Number.isFinite(x)).sort((x, y) => x - y);
  if (all.length <= limit) return all;
  const out = [];
  for (let i = 0; i < limit; i++) out.push(all[Math.round((i * (all.length - 1)) / (limit - 1))]);
  return [...new Set(out)];
}

/** `paintPlaybackSlide` of the viewer (universal-slides-viewer.ts) on the recorder: the hooks as the viewer wires them. */
async function paintPlayback(slide, pres, ps, animMs, view, frameNo) {
  const bg = new Set(slide.timing?.animBg ?? []);
  const actx = { animBg: (spid) => bg.has(spid) };
  const mediaInfo = new Map((slide.timing?.media ?? []).map((m) => [m.spid, m]));
  const hasMedia = slide.shapes.some((sh) => sh.media || sh.image?.mime === 'image/gif');
  const opts = {
    shapeAnim: (sh) => (sh.inherited || !ps.effects.length ? null
      : core.shapeAnimHooks(ps.effects, sh.groupIds ? [sh.id, ...sh.groupIds] : [sh.id], animMs, sh.dgmId ? { ...actx, dgmId: sh.dgmId } : actx)),
    ...(hasMedia ? {
      mediaFrame: (sh, i) => (sh.inherited ? null : (sh.media || sh.image?.mime === 'image/gif') ? mediaStub(i, frameNo) : null),
      shapeOpacity: (sh) => {
        const mi = sh.media ? mediaInfo.get(sh.id) : undefined;
        return mi?.showWhenStopped === false && !core.mediaStateAt(ps.effects, sh.id, animMs, {}, !!mi.loop).playing ? 0 : 1;
      },
    } : {}),
  };
  const r = await recordTsSlide({ tsRoot: root, slide, pres, scale: view.scale, ox: view.ox, oy: view.oy, opts });
  return { ...r, hasMedia };
}

const writer = new CaptureWriter(['Slides draw-op streams of animated frames (B4b): the frozen TS painter with the viewer\'s hooks at sampled times of the default timeline; see js/capture-anim.mjs']);
writer.meta('ts-root-commit', tsCommit(root));
writer.meta('stream', 'record-ctx.mjs canonical op stream in the DRAW form (lib/opstream.mjs); per frame `lines:sha1[0..16]`');
const streams = [];
let nDecks = 0, nFrames = 0, nErr = 0, nLines = 0;
const t0 = Date.now();

async function record(key, bytes) {
  if (only && !key.includes(only)) return;
  if (nDecks >= max) return;
  let pkg, pres;
  try { pkg = core.PptxPackage.load(bytes); pres = core.readPresentation(pkg); } catch { return; }
  const slides = pres.slides.map((ref) => { try { return ref.target === undefined ? undefined : core.readSlide(pkg, ref.target); } catch { return undefined; } });
  const models = slides.map((s) => s?.timing);
  const tl = core.buildPlaybackTimeline(models, {});
  if (!tl.slides.some((ps) => ps.effects.length)) return;
  nDecks++;
  streams.push(`#deck ${key}\n`);
  const v = nDecks % 2 === 0 ? 'a' : 'b';
  const view = VIEWS[v](pres);
  const size = `${Math.ceil(pres.slideW * view.scale + 2 * view.ox)}x${Math.ceil(pres.slideH * view.scale + 2 * view.oy)}`;
  let frameNo = 0;
  for (const ps of tl.slides) {
    if (!ps.effects.length || !slides[ps.index]) continue;
    for (const ms of animTimes(ps, per)) {
      const t = ps.animStartMs + ms;
      const st = core.playbackStateAt(tl, t);
      if (!st.slide || st.slide.index !== ps.index || !slides[ps.index]) continue;
      frameNo++;
      const r = await paintPlayback(slides[ps.index], pres, st.slide, st.animMs, view, frameNo);
      const d = await digest(r.text);
      nFrames++; nLines += d.lines;
      if (r.error) nErr++;
      streams.push(`#frame ${ps.index} t=${t} anim=${st.animMs} n=${frameNo} view=${v} scale=${view.scale} ox=${view.ox} oy=${view.oy} canvas=${size} media=${r.hasMedia ? 1 : 0} lines=${d.lines} sha=${d.sha}\n`);
      if (r.error) streams.push(`#error ${oneLine(r.error)}\n`);
      streams.push(d.text);
      writer.set(key, `f${frameNo}`, r.error ? `err:${oneLine(r.error)}` : `${d.lines}:${d.sha}`);
    }
  }
}

let mode;
if (a.opt('--gen')) {
  const decksDir = a.opt('--decks');
  for (const group of a.opt('--gen').split(',')) {
    mode = group;
    const n = GROUPS[group].count;
    for (let i = 0; i < n; i++) {
      const f = decksDir ? join(decksDir, group, deckFile(group, i)) : null;
      const bytes = f && existsSync(f) ? new Uint8Array(readFileSync(f)) : new Uint8Array(buildGroupDeck(group, i).bytes);
      await record(groupKey(group, i), bytes);
    }
  }
} else if (a.flag('--samples')) {
  mode = 'samples';
  for (const rel of trackedSamples()) if (/\.pptx$/i.test(rel)) await record(`samples/${rel}`, readSample(rel));
} else {
  console.error('usage: capture-anim.mjs (--samples | --gen <group[,group]>) [--out stem] [--only s] [--max n] [--per n]');
  process.exit(2);
}
const stem = a.opt('--out', join(capturesDir, `anim-${mode}`));
mkdirSync(dirname(stem), { recursive: true });
writeFileSync(`${stem}.capture`, writer.text());
writeFileSync(`${stem}.ops.gz`, gzipSync(Buffer.from(streams.join(''), 'utf8'), { level: 9 }));
console.log(`${mode}: ${nDecks} decks, ${nFrames} frames, ${nLines} draw lines, ${nErr} errors, ${((Date.now() - t0) / 1000).toFixed(1)} s -> ${stem}.*`);
