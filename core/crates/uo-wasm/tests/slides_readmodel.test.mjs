// Wave 4: the read model of the Slides module (src/slides.rs "Read model", js/slides.mjs) against the frozen TS model it replaces in
// `<universal-slides-viewer engine="rust">`: per slide the accessible outline (`SlideA11y` input), the link boxes (the TS viewer's
// `hitTestHyperlink` geometry), the pictures to decode and the media/GIF shapes (`warmSlideImages`, `MediaPlayer`), per deck the embedded
// fonts, the typefaces and the show settings, and the media states of the playback frames (`mediaStateAt` over the TS timeline) —
// over the tracked samples and the committed generated decks. Node only (it imports the frozen TS `slides-core` dist).
//
//   cargo build -p uo-wasm --features slides --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> [UO_PPT_WASM_PATH=<uo-slides-ppt.wasm>] [UO_TS_ROOT=<frozen checkout>] [UO_RM_DECKS=<n per group, default 60>]
//     node --test crates/uo-wasm/tests/slides_readmodel.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { bindSlides, bindSlidesPpt } from '../js/slides.mjs';
import { loadSlidesCore, readSample, trackedSamples, tsRoot } from '../../uo-pptx/js/lib/common.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const uo = bindSlides((await WebAssembly.instantiate(readFileSync(wasmPath), {})).instance.exports);
const pptPath = process.env.UO_PPT_WASM_PATH ?? join(here, '../../../target/b7/uo-slides-ppt.wasm');
const ppt = existsSync(pptPath) ? bindSlidesPpt((await WebAssembly.instantiate(readFileSync(pptPath), {})).instance.exports) : null;
const core = await loadSlidesCore(tsRoot({ opt: (_n, d) => d, has: () => false }));
const isCfb = (b) => b[0] === 0xd0 && b[1] === 0xcf && b[2] === 0x11 && b[3] === 0xe0;
const DECKS_DIR = join(here, '../../uo-pptx/tests/captures/decks');
const N = Number(process.env.UO_RM_DECKS ?? 60);

const decks = [];
for (const rel of trackedSamples()) if (/\.(pptx|ppt)$/i.test(rel) && (!isCfb(readSample(rel)) || ppt)) decks.push([`samples/${rel}`, () => readSample(rel)]);
for (const g of ['b1a', 'b5b', 'b6a', 'c1d']) {
  const dir = join(DECKS_DIR, g);
  if (!existsSync(dir)) continue;
  for (const f of readdirSync(dir).filter((n) => n.endsWith('.pptx')).sort().slice(0, N)) decks.push([`${g}/${f}`, () => new Uint8Array(readFileSync(join(dir, f)))]);
}

/** The TS viewer's view of a deck (what `loadPptx` read), or null when the TS refuses the file. */
function tsDeck(bytes) {
  let pkg, pres;
  try { pkg = core.PptxPackage.load(bytes); pres = core.readPresentation(pkg); } catch { return null; }
  const slides = [];
  for (const ref of pres.slides) {
    if (!ref.target) continue;
    try { slides.push(core.readSlide(pkg, ref.target)); } catch { slides.push({ shapes: [] }); }
  }
  return { pkg, pres, slides };
}

const joinRuns = (p) => p.runs.map((r) => r.text).join('');
/** `a11yShapesOf` of the viewer (slides/packages/viewer/src/a11y.ts), written here over the frozen model. */
const tsOutline = (slide) => slide.shapes.map((s) => ({
  inherited: !!s.inherited, image: !!s.image, media: !!s.media, phType: s.phType, descr: s.descr, hasText: !!s.txBody,
  paragraphs: (s.txBody?.paragraphs ?? []).map(joinRuns),
  table: s.table ? s.table.rows.map((row) => row.cells.map((c) => ({ hMerge: !!c.hMerge, vMerge: !!c.vMerge, paragraphs: (c.txBody?.paragraphs ?? []).map(joinRuns) }))) : null,
}));
/** `warmSlideImages` (and `readyImages`): the parts in order, each once. */
function tsImages(s) {
  const out = [];
  const push = (r, gif = false) => { if (r && !out.some((o) => o.part === r.part)) out.push({ part: r.part, mime: r.mime, gif }); };
  push(s.bgImage);
  for (const sh of s.shapes) {
    if (sh.image) push(sh.image, sh.image.mime === 'image/gif');
    if (sh.fillImage) push(sh.fillImage);
    if (sh.table) for (const r of core.tableImages(sh.table)) push(r);
    for (const r of Object.values(sh.chartImages ?? {})) push(r);
    for (const p of sh.txBody?.paragraphs ?? []) {
      if (p.bulletImage) push(p.bulletImage);
      for (const r of p.runs) if (r.fillImage) push(r.fillImage);
    }
  }
  return out;
}
const VIEW = { scale: 1280 / 12192000, ox: 7, oy: 11 };
const sha = (b) => createHash('sha256').update(b).digest('hex').slice(0, 16);

test('the read model equals the frozen TS model: outline, links, pictures, media, fonts, typefaces, show settings', () => {
  const tot = { decks: 0, slides: 0, shapes: 0, links: 0, images: 0, special: 0, media: 0, fonts: 0, typefaces: 0, diffs: [] };
  const diff = (where, what) => { if (tot.diffs.length < 20) tot.diffs.push(`${where}: ${what}`); else tot.diffs.length++; };
  for (const [key, get] of decks) {
    const bytes = get();
    const ts = tsDeck(bytes);
    if (!ts) continue;
    const d = uo.open(isCfb(bytes) ? ppt.convert(bytes) : bytes);
    try {
      tot.decks++;
      assert.equal(d.count, ts.slides.length, `${key}: count`);
      // per deck
      const fr = d.fonts().map((f) => ({ family: f.family, variant: f.variant, weight: f.weight, italic: f.italic, data: sha(f.data) }));
      const ft = core.readEmbeddedFonts(ts.pkg).map((f) => ({ family: f.family, variant: f.variant, weight: f.weight, italic: f.italic, data: sha(f.data) }));
      if (JSON.stringify(fr) !== JSON.stringify(ft)) diff(key, `fonts ${JSON.stringify(fr)} != ${JSON.stringify(ft)}`);
      tot.fonts += ft.length;
      const tr = d.typefaces(), tt = core.collectDeckTypefaces(ts.pkg);
      if (JSON.stringify(tr) !== JSON.stringify(tt)) diff(key, `typefaces ${tr} != ${tt}`);
      tot.typefaces += tt.length;
      const sr = d.showSettings(), st = ts.pres.show ?? null;
      if (JSON.stringify(sr) !== JSON.stringify(st)) diff(key, `show settings ${JSON.stringify(sr)} != ${JSON.stringify(st)}`);
      ts.slides.forEach((s, i) => {
        tot.slides++;
        const w = `${key}#${i}`;
        // outline
        const o = d.outline(i), ot = tsOutline(s);
        tot.shapes += ot.length;
        if (JSON.stringify(o) !== JSON.stringify(ot)) diff(w, 'outline differs');
        // links: the TS hit-test boxes are the RunUrl boxes, in shape order
        const lr = d.links(i, VIEW.scale, VIEW.ox, VIEW.oy).filter((b) => b.source === 'run').map((b) => [b.shape, b.x, b.y, b.w, b.h, b.target.url]);
        const lt = [];
        s.shapes.forEach((sh, k) => {
          if (!sh.xfrm || !sh.txBody) return;
          let url = null;
          for (const p of sh.txBody.paragraphs) for (const r of p.runs) if (url === null && r.hyperlink) url = r.hyperlink;
          if (url !== null) lt.push([k, VIEW.ox + sh.xfrm.x * VIEW.scale, VIEW.oy + sh.xfrm.y * VIEW.scale, sh.xfrm.cx * VIEW.scale, sh.xfrm.cy * VIEW.scale, url]);
        });
        tot.links += lt.length;
        if (JSON.stringify(lr) !== JSON.stringify(lt)) diff(w, `links ${JSON.stringify(lr)} != ${JSON.stringify(lt)}`);
        // pictures
        const a = d.assets(i);
        const ir = a.images.map((x) => ({ part: x.part, mime: x.mime, gif: x.gif }));
        const it = tsImages(s);
        tot.images += it.length;
        tot.special += a.images.filter((x) => x.special).length;
        if (JSON.stringify(ir) !== JSON.stringify(it)) diff(w, `images ${JSON.stringify(ir)} != ${JSON.stringify(it)}`);
        for (const x of a.images) {
          const b = ts.pkg.getPart(x.part);
          const special = !!b && (core.isMetafile?.(b) || core.isPict?.(b) || core.isTiff?.(b));
          if (core.isMetafile && special !== x.special) diff(w, `special ${x.part}`);
        }
        // media and GIF shapes
        const mr = a.media.map((m) => ({ shape: m.shape, inherited: m.inherited, spid: m.spid, gif: m.gifPart, media: m.media, info: m.info }));
        const infos = new Map((s.timing?.media ?? []).map((m) => [m.spid, m]));
        const mt = [];
        s.shapes.forEach((sh, k) => {
          const gif = sh.image?.mime === 'image/gif' ? sh.image.part : null;
          if (!sh.media && !gif) return;
          const m = sh.media ? { kind: sh.media.kind, mime: sh.media.mime } : null;
          if (m && sh.media.part) m.part = sh.media.part;
          if (m && sh.media.url) m.url = sh.media.url;
          if (m && sh.media.trim) m.trim = sh.media.trim;
          if (m && sh.media.fade) m.fade = sh.media.fade;
          const inf = infos.get(sh.id);
          const info = inf ? Object.fromEntries(['spid', 'kind', 'vol', 'mute', 'showWhenStopped', 'rewind', 'loop', 'fullScreen'].filter((x) => inf[x] !== undefined).map((x) => [x, inf[x]])) : null;
          mt.push({ shape: k, inherited: !!sh.inherited, spid: sh.id, gif, media: m, info });
        });
        tot.media += mt.length;
        const norm = (v) => JSON.stringify(v, (k2, x) => (x && typeof x === 'object' && !Array.isArray(x) ? Object.fromEntries(Object.entries(x).sort()) : x));
        if (norm(mr) !== norm(mt)) diff(w, `media ${norm(mr)} != ${norm(mt)}`);
      });
    } finally {
      d.free();
    }
  }
  console.log(`read model: ${tot.decks} decks, ${tot.slides} slides, ${tot.shapes} shapes outlined, ${tot.links} link boxes, ${tot.images} pictures (${tot.special} rasterized by Rust), ${tot.media} media/GIF shapes, ${tot.fonts} embedded fonts, ${tot.typefaces} typefaces; ${tot.diffs.length} differences`);
  assert.deepEqual(tot.diffs, []);
  assert.ok(tot.decks > 100 && tot.links > 0 && tot.media > 0);
});

test('the media states of the playback frames equal the TS `mediaStateAt` over the TS timeline', () => {
  let frames = 0, shapes = 0;
  const bad = [];
  for (const [key, get] of decks) {
    const bytes = get();
    const ts = tsDeck(bytes);
    if (!ts || !ts.slides.some((s) => s.shapes.some((sh) => sh.media && !sh.inherited))) continue;
    const d = uo.open(isCfb(bytes) ? ppt.convert(bytes) : bytes);
    for (const opts of [{}, { tickMs: 1000 / 30 }]) {
      const tl = core.buildPlaybackTimeline(ts.slides.map((s) => s.timing), opts);
      const rtl = d.newTimeline({ tickMs: opts.tickMs ?? NaN });
      const clock = tl.tickMs > 0 ? { latencyMs: 2 * tl.tickMs, scale: tl.mediaClock } : {};
      const steps = 40;
      for (let k = 0; k <= steps; k++) {
        const t = (tl.durationMs * k) / steps;
        const st = core.playbackStateAt(tl, t);
        for (const which of [0, 1]) {
          const ps = which === 0 ? st.slide : st.from;
          const animMs = which === 0 ? st.animMs : Infinity;
          const r = d.mediaStates(rtl, t, which, clock);
          frames++;
          if (!ps) { if (r !== null) bad.push(`${key} t=${t} w=${which}: rust has a slide`); continue; }
          if (!r || r.index !== ps.index) { bad.push(`${key} t=${t} w=${which}: index`); continue; }
          const slide = ts.slides[ps.index];
          const gifT = Number.isFinite(animMs) ? animMs : ps.endMs - ps.animStartMs;
          if (r.gifMs !== gifT || !Object.is(r.animMs, animMs)) bad.push(`${key} t=${t} w=${which}: clock ${r.gifMs}/${gifT}`);
          const infos = new Map((slide.timing?.media ?? []).map((m) => [m.spid, m]));
          slide.shapes.forEach((sh, i) => {
            if (sh.inherited || !sh.media) return;
            shapes++;
            const want = core.mediaStateAt(ps.effects, sh.id, animMs, clock, !!infos.get(sh.id)?.loop);
            const got = r.states.get(i);
            const key2 = (v) => JSON.stringify(v && Object.fromEntries(Object.entries(v).sort()));
            if (key2(got) !== key2(want)) bad.push(`${key} t=${t} w=${which} shape ${i}: ${JSON.stringify(got)} != ${JSON.stringify(want)}`);
          });
        }
      }
      rtl.free();
    }
    d.free();
  }
  console.log(`media states: ${frames} stage frames, ${shapes} media-shape states compared, ${bad.length} different`);
  assert.deepEqual(bad.slice(0, 10), []);
  assert.ok(shapes > 0);
});

test('read-model queries on bad input: no such slide, NaN view, a freed deck', () => {
  const [, get] = decks.find(([k]) => k.startsWith('b1a/')) ?? decks[0];
  const d = uo.open(get());
  assert.throws(() => d.outline(10_000));
  assert.throws(() => d.assets(10_000));
  assert.throws(() => d.links(0, NaN, 0, 0));
  assert.throws(() => d.links(0, -1, 0, 0));
  d.free();
  assert.throws(() => d.outline(0));
});
