// Parity capture of the editing session (plan section 4.3, frontier B8): seeded scripts of editing commands over the
// committed decks, run on the FROZEN TS operations (`deck-edit.ts`, `xml-edit.ts`, `history.ts`, `blank-deck.ts` of
// slides/packages/slides, and `writePptxEmbedding` of shared/fonts) with the `edit()` / `undo()` / `redo()` protocol of
// `universal-slides.ts` (`Session` below: the same sequence snapshot -> operation -> changedSince -> re-read -> select ->
// history.push, and the same wrappers for what the element resolves from its UI state; the DOM stays out). The Rust replay
// (`tests/edit_replay.rs`) executes the same scripts through `uo_pptx::edit::Editor`.
//
//   node core/crates/uo-pptx/js/capture-edit.mjs --samples                       # the tracked .pptx samples
//   node core/crates/uo-pptx/js/capture-edit.mjs --gen b1a,b6a --decks core/crates/uo-pptx/tests/captures/decks
//   node core/crates/uo-pptx/js/capture-edit.mjs --blank                         # blank decks (byte-equal parts) + long scripts
//   node core/crates/uo-pptx/js/capture-edit.mjs --fonts                         # save with the "fonts in the file" option
//   options: --out <stem> (default tests/captures/edit-<mode>)  --steps N (ops per script, default 60)  --n N (decks per group)
//            --only <s>  --ts-root <dir>  --decks <dir>  --dump <key>#<step> (print the TS part texts changed by that step)
//
// Records: key `<deck>#<step>` (step 0 = the opened deck, before any operation), facets:
//   op      the operation name        ret    `none` | `b0` | `b1` | `n<number>` | `s<text>` | `err` (the TS threw)
//   parts   `<count>:<sha256[0..32]>` over every part in package order (name, length, SHA-256 of the bytes)
//   st      `i=<slide index>;sel=<shape id|->;u=<canUndo>;r=<canRedo>;ul=<undo label|->;rl=<redo label|->;d=<dirty>`
//   pres    `len:sha256[0..32]` of the canonical JSON of readPresentation
//   cur     the same of `{notes, slide}` of the current slide (`-` when there is no slide, `err` when readSlide throws)
// Sidecars: `<stem>.ops.gz` (one line per step: `key<TAB>op<TAB>token...`; tokens `s:<pct-encoded>`, `n:<f64 bits>`,
// `b:0|1`, `h:<hex>`, `u` undefined, `N` null; the script is replayed from it, so the Rust side has no generator) and
// `<stem>.parts.gz` (per step the parts that changed: `key<TAB>name<TAB>len:sha256[0..16]` or `-`).
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { gzipSync } from 'node:zlib';
import { CaptureWriter, Rng } from '../../uo-testkit/js/testkit.mjs';
import { bytesDigest, canonJson, textDigest } from './lib/canon-json.mjs';
import { args, capturesDir, loadSlidesCore, readSample, repoDir, trackedSamples, tsCommit, tsRoot } from './lib/common.mjs';
import { GROUPS, deckFile, groupKey } from './lib/gen/groups.mjs';

const a = args();
const root = tsRoot(a);
const core = await loadSlidesCore(root);
const editDir = join(root, 'slides', 'packages', 'slides', 'dist');
const imp = (p) => import(pathToFileURL(p).href);
const deckEdit = await imp(join(editDir, 'deck-edit.js'));
const { EditHistory, changedSince, restore, snapshot } = await imp(join(editDir, 'history.js'));
const { blankDeck } = await imp(join(editDir, 'blank-deck.js'));
const fontsDist = join(root, 'shared', 'fonts', 'dist');
const fontsOoxml = await imp(join(fontsDist, 'ooxml.js'));
const fontsEmbedding = await imp(join(fontsDist, 'embedding.js'));

// ───────────────────────── deterministic environment ─────────────────────────

const FIXED_NOW = Date.UTC(2026, 0, 2, 3, 4, 0);
const RealDate = Date;
// eslint-disable-next-line no-global-assign
Date = class extends RealDate { constructor(...x) { if (x.length) super(...x); else super(FIXED_NOW); } static now() { return FIXED_NOW; } };
/** `Math.random` of the TS side: the xorshift32 of `uo_pptx::edit::Xorshift32` (seed 1 at the start of every script). */
let rs = 1;
const reseed = () => { rs = 1; };
Math.random = () => { rs ^= rs << 13; rs >>>= 0; rs ^= rs >>> 17; rs ^= rs << 5; rs >>>= 0; return rs / 4294967296; };

// ───────────────────────── encodings shared with the Rust replay ─────────────────────────

const dv = new DataView(new ArrayBuffer(8));
const bits = (x) => { x = +x; if (Number.isNaN(x)) return '7ff8000000000000'; dv.setFloat64(0, x); return dv.getUint32(0).toString(16).padStart(8, '0') + dv.getUint32(4).toString(16).padStart(8, '0'); };
const fixLone = (s) => s.replace(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g, '�');
const esc = (s) => encodeURIComponent(fixLone(String(s))).replace(/[!'()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
const sha = (u8) => createHash('sha256').update(u8).digest('hex');

function tok(v) {
  if (v === undefined) return 'u';
  if (v === null) return 'N';
  if (typeof v === 'string') return `s:${esc(v)}`;
  if (typeof v === 'number') return `n:${bits(v)}`;
  if (typeof v === 'boolean') return `b:${v ? 1 : 0}`;
  if (v instanceof Uint8Array) return `h:${Buffer.from(v).toString('hex')}`;
  throw new Error(`token of ${typeof v}`);
}

/** Operation schemas: the order of the fields on the sidecar line (kinds: plain value, `frame`, `spec`, `hit`, `target`). */
const SCHEMA = {
  addSlide: ['layout'], duplicateSlide: ['i'], deleteSlide: ['i'], moveSlide: ['from', 'to'], hideSlide: ['i', 'hidden'], layout: ['i', 'layout'],
  background: ['target', 'color'], transition: ['target', ['spec']], showSettings: ['loop', 'useTimings', 'showAnimation', 'kiosk'],
  text: ['shape', 'text'], typing: ['slide', 'shape', 'text'],
  format: ['shape', 'bold', 'italic', 'underline', 'strike', 'size', 'font', 'color'], grow: ['shape', 'dir', 'base'],
  align: ['shape', 'algn'], bullets: ['shape', 'kind'], clearFormat: ['shape'], link: ['shape', 'url'],
  textBox: ['text', ['frame']], shape: ['prst', ['frame']], image: ['bytes', 'mime', ['frame'], 'sizeW', 'sizeH'],
  frame: ['shape', 'x', 'y', 'cx', 'cy'], fill: ['shape', 'color'], outline: ['shape', 'color'], arrange: ['shape', 'mode'], deleteShape: ['shape'],
  copy: ['shape'], paste: [], xpaste: ['deck', 'slide', 'shape'], notes: ['i', 'text'],
  properties: ['title', 'subject', 'author', 'keywords', 'description', 'lastModifiedBy'],
  replace: ['query', 'replacement', 'matchCase', ['hit']],
  select: ['shape'], goto: ['i'], undo: [], redo: [], markClean: [], embed: ['embed', 'subset'], saveFonts: [],
};

function ser(op) {
  const out = [op.op];
  for (const f of SCHEMA[op.op]) {
    if (Array.isArray(f)) {
      const k = f[0];
      const v = op[k];
      if (k === 'frame') out.push(v ? `f:${bits(v.x)},${bits(v.y)},${bits(v.cx)},${bits(v.cy)}` : 'u');
      else if (k === 'spec') {
        if (v === null) out.push('N');
        else out.push(tok(v.type), tok(v.dir), tok(v.speed), tok(v.advanceOnClick), tok(v.advanceAfterMs));
      } else if (k === 'hit') out.push(...(v ? [tok(v.slide), tok(v.shape)] : ['u', 'u']));
    } else if (f === 'target') out.push(op.target === 'all' ? 'A' : tok(op.target));
    else out.push(tok(op[f]));
  }
  return out;
}

// ───────────────────────── facets ─────────────────────────

const partsDigest = (pkg) => {
  const h = createHash('sha256');
  const names = pkg.partNames();
  for (const n of names) {
    const b = pkg.getPart(n);
    h.update(`${n}\0${b.length}\0`);
    h.update(createHash('sha256').update(b).digest());
  }
  return `${names.length}:${h.digest('hex').slice(0, 32)}`;
};
const partMap = (pkg) => new Map(pkg.partNames().map((n) => { const b = pkg.getPart(n); return [n, `${b.length}:${sha(b).slice(0, 16)}`]; }));

function curRecord(pkg, index) {
  let pres;
  try { pres = core.readPresentation(pkg); } catch { return { pres: 'err', cur: 'err' }; }
  const presText = textDigest(canonJson(pres));
  const refs = pres.slides.filter((r) => r.target);
  const ref = refs[index];
  if (!ref) return { pres: presText, cur: '-' };
  try {
    const slide = core.readSlide(pkg, ref.target);
    let notes = '';
    try { notes = core.readSlideNotes(pkg, ref.target); } catch (e) { notes = `\u0000error:${String(e?.message ?? e).slice(0, 160)}`; }
    return { pres: presText, cur: textDigest(canonJson({ notes, slide })) };
  } catch { return { pres: presText, cur: 'err' }; }
}

// ───────────────────────── the session (universal-slides.ts, without the DOM) ─────────────────────────

const FONT_STEPS = [8, 9, 10, 10.5, 11, 12, 14, 16, 18, 20, 24, 28, 32, 36, 40, 44, 48, 54, 60, 66, 72, 80, 88, 96];
const EMU_PER_PX = 9525;
let sessionIds = 0;
/** The other packages `xpaste` copies from (generated decks with pictures, tables, groups). */
const XDECKS = ['gen/b1a/0003', 'gen/b1a/0004', 'gen/b1a/0006', 'gen/b1a/0010'];
const xcache = new Map();
const xsession = (key) => {
  let s = xcache.get(key);
  if (!s) {
    const [, group, n] = key.split('/');
    s = new Session(new Uint8Array(readFileSync(join(capturesDir, 'decks', group, `${n}.pptx`))));
    xcache.set(key, s);
  }
  return s;
};

class Session {
  constructor(bytes, lang = 'en-US') {
    this.pkg = core.PptxPackage.load(bytes);
    this.history = new EditHistory(100);
    this.id = ++sessionIds;
    this.index = 0;
    this.selected = null; // shape id
    this.lang = lang;
    this.clip = null;
    this.embedding = { embed: false, subset: true, skipCommonFonts: true, ...fontsOoxml.readPptxEmbedding(this.pkg).settings };
    this.embeddingTouched = false;
  }

  pres() { return core.readPresentation(this.pkg); }
  paths() { return this.pres().slides.filter((r) => r.target).map((r) => r.target); }
  slides() {
    return this.paths().map((p) => { try { return core.readSlide(this.pkg, p); } catch { return { shapes: [] }; } });
  }
  slideCount() { return this.paths().length; }
  slidePath(i = this.index) {
    const p = this.paths()[i];
    if (!p) throw new Error(`slide ${i} inexistente`);
    return p;
  }
  hasSlide(i) { return !!this.paths()[i]; }
  editState() { return { index: this.index, shapeId: this.selected }; }
  selectById(id) {
    if (id === null) { this.selected = null; return; }
    const s = this.slides()[this.index];
    this.selected = s && s.shapes.some((sh) => !sh.inherited && sh.id === id) ? id : null;
  }
  rereadDeck(index) {
    const n = this.slideCount();
    this.index = Math.max(0, Math.min(index, n - 1));
  }

  edit(label, fn, reread = 'slide', then = {}) {
    const pkg = this.pkg;
    const before = snapshot(pkg);
    const stateBefore = this.editState();
    const result = fn(pkg);
    if (!changedSince(pkg, before)) return result;
    const keep = then.select !== undefined ? then.select : stateBefore.shapeId;
    if (reread === 'deck') this.rereadDeck(then.index ?? this.index);
    if (then.index !== undefined && reread !== 'deck') this.index = then.index;
    this.selectById(keep);
    this.history.push(label, before, snapshot(pkg), stateBefore, this.editState());
    return result;
  }

  undo() {
    const u = this.history.undo();
    if (!u) return undefined;
    restore(this.pkg, u.snap);
    this.rereadDeck(u.state.index);
    this.selectById(u.state.shapeId);
    return undefined;
  }
  redo() {
    const r = this.history.redo();
    if (!r) return undefined;
    restore(this.pkg, r.snap);
    this.rereadDeck(r.state.index);
    this.selectById(r.state.shapeId);
    return undefined;
  }

  slideSize() { const p = this.pres(); return { W: p.slideW || 12192000, H: p.slideH || 6858000 }; }
  centered(w, h) {
    const { W, H } = this.slideSize();
    const cx = Math.min(w, W * 0.9), cy = Math.min(h, H * 0.9);
    return { x: (W - cx) / 2, y: (H - cy) / 2, cx, cy };
  }

  defaultNewLayout() {
    const layouts = deckEdit.listLayouts(this.pkg);
    const sp = this.paths()[this.index];
    const cur = sp ? deckEdit.layoutOf(this.pkg, sp) : undefined;
    const info = layouts.find((l) => l.path === cur);
    if (info?.type === 'title') {
      const next = layouts.find((l) => l.master === info.master && l.type === 'obj') ?? layouts.find((l) => l.master === info.master && l.type === 'tx');
      if (next) return next.path;
    }
    return cur ?? layouts.find((l) => l.type === 'obj')?.path ?? layouts[0]?.path;
  }

  insertWith(label, fn) {
    if (!this.slideCount()) this.run({ op: 'addSlide', layout: undefined });
    const path = this.slidePath();
    const id = this.edit(label, (pkg) => fn(pkg, path), 'slide', { select: null });
    this.selectById(id);
    return `s${id}`;
  }

  findText(query, matchCase) {
    if (!query) return [];
    const q = matchCase ? query : query.toLowerCase();
    const out = [];
    this.slides().forEach((s, si) => s.shapes.forEach((sh) => {
      if (sh.inherited || !sh.txBody) return;
      const text = sh.txBody.paragraphs.map((p) => p.runs.map((r) => r.text).join('')).join('\n');
      if ((matchCase ? text : text.toLowerCase()).includes(q)) out.push({ slide: si, shape: sh.id });
    }));
    return out;
  }

  inGroup(id) {
    const sh = this.slides()[this.index]?.shapes.find((s) => s.id === id && !s.inherited);
    return !!sh?.groupIds?.length;
  }

  /** Runs one command; returns the `ret` facet value. */
  run(op) {
    const v = this.exec(op);
    if (v === undefined) return 'none';
    if (typeof v === 'boolean') return `b${v ? 1 : 0}`;
    if (typeof v === 'number') return `n${v}`;
    return v;
  }

  exec(o) {
    const { lang } = this;
    const cur = this.index;
    const onShape = (label, fn) => { const p = this.slidePath(); return this.edit(label, (pkg) => fn(pkg, p)); };
    switch (o.op) {
      case 'addSlide': {
        const at = this.slideCount() ? cur + 1 : 0;
        const path = o.layout ?? this.defaultNewLayout();
        if (!path) return -1;
        this.edit('newSlide', (pkg) => { deckEdit.addSlide(pkg, at, path, lang); }, 'deck', { index: at, select: null });
        return at;
      }
      case 'duplicateSlide': {
        if (!this.hasSlide(o.i)) return -1;
        this.edit('duplicateSlide', (pkg) => { deckEdit.duplicateSlide(pkg, o.i); }, 'deck', { index: o.i + 1, select: null });
        return o.i + 1;
      }
      case 'deleteSlide': {
        if (!this.hasSlide(o.i)) return undefined;
        const next = Math.max(0, Math.min(o.i, this.slideCount() - 2));
        this.edit('deleteSlide', (pkg) => deckEdit.deleteSlide(pkg, o.i), 'deck', { index: next, select: null });
        return undefined;
      }
      case 'moveSlide': {
        if (o.from === o.to || !this.hasSlide(o.from) || o.to < 0 || o.to >= this.slideCount()) return undefined;
        const index = cur === o.from ? o.to : cur;
        this.edit('moveSlide', (pkg) => deckEdit.moveSlide(pkg, o.from, o.to), 'deck', { index });
        return undefined;
      }
      case 'hideSlide': {
        if (!this.hasSlide(o.i)) return undefined;
        const p = this.slidePath(o.i);
        this.edit('hideSlide', (pkg) => deckEdit.setSlideHidden(pkg, p, o.hidden), o.i);
        return undefined;
      }
      case 'layout': {
        if (!this.hasSlide(o.i)) return undefined;
        const p = this.slidePath(o.i);
        this.edit('layout', (pkg) => deckEdit.setSlideLayout(pkg, p, o.layout), o.i);
        return undefined;
      }
      case 'background': {
        const targets = o.target === 'all' ? this.paths() : [this.slidePath(o.target)];
        this.edit('background', (pkg) => { for (const p of targets) deckEdit.setSlideBackground(pkg, p, o.color); }, o.target === 'all' ? 'deck' : o.target);
        return undefined;
      }
      case 'transition': {
        const targets = o.target === 'all' ? this.paths() : [this.slidePath(o.target)];
        this.edit('transition', (pkg) => { for (const p of targets) deckEdit.setSlideTransition(pkg, p, o.spec); }, o.target === 'all' ? 'deck' : o.target);
        return undefined;
      }
      case 'showSettings': {
        const s = {};
        for (const k of ['loop', 'useTimings', 'showAnimation', 'kiosk']) if (o[k] !== undefined) s[k] = o[k];
        this.edit('showSettings', (pkg) => deckEdit.writeShowSettings(pkg, s), 'deck');
        return undefined;
      }
      case 'text': return onShape('text', (pkg, p) => deckEdit.setShapeText(pkg, p, o.shape, o.text, lang));
      case 'typing': {
        const p = this.paths()[o.slide];
        const sh = this.slides()[o.slide]?.shapes.find((s) => s.id === o.shape && !s.inherited);
        const before = sh ? (sh.txBody?.paragraphs ?? []).map((q) => q.runs.map((r) => r.text).join('')).join('\n') : '';
        if (!sh || !p || o.text === before) return undefined;
        const prev = this.index;
        this.index = o.slide;
        try { return this.edit('typing', (pkg) => deckEdit.setShapeText(pkg, p, sh.id, o.text, lang), o.slide); } finally { this.index = prev; }
      }
      case 'format': {
        const f = {};
        for (const k of ['bold', 'italic', 'underline', 'strike', 'size', 'font', 'color']) if (o[k] !== undefined) f[k] = o[k];
        return onShape('format', (pkg, p) => deckEdit.formatShapeText(pkg, p, o.shape, f, lang));
      }
      case 'grow': {
        const step = (pt) => {
          const v = pt ?? o.base;
          if (o.dir > 0) return FONT_STEPS.find((s) => s > v + 0.01) ?? Math.round(v + 10);
          return [...FONT_STEPS].reverse().find((s) => s < v - 0.01) ?? Math.max(1, v - 1);
        };
        return onShape('fontSize', (pkg, p) => deckEdit.scaleShapeText(pkg, p, o.shape, step, lang));
      }
      case 'align': return onShape('align', (pkg, p) => deckEdit.alignShapeText(pkg, p, o.shape, o.algn));
      case 'bullets': return onShape('bullets', (pkg, p) => deckEdit.setShapeBullets(pkg, p, o.shape, o.kind));
      case 'clearFormat': return onShape('clearFormat', (pkg, p) => deckEdit.clearShapeFormatting(pkg, p, o.shape, lang));
      case 'link': return onShape('link', (pkg, p) => deckEdit.setShapeLink(pkg, p, o.shape, o.url, lang));
      case 'textBox': {
        const { W } = this.slideSize();
        const f = o.frame ?? this.centered(W * 0.4, 12700 * 40);
        return this.insertWith('textBox', (pkg, p) => deckEdit.insertTextBox(pkg, p, f, o.text, lang));
      }
      case 'shape': {
        const { H } = this.slideSize();
        const side = Math.round(H * 0.3);
        const f = o.frame ?? (o.prst === 'line' ? { ...this.centered(side * 1.5, 0), cy: 0 } : this.centered(o.prst === 'rect' || o.prst === 'roundRect' ? side * 1.5 : side, side));
        return this.insertWith('shape', (pkg, p) => deckEdit.insertShape(pkg, p, o.prst, f, lang));
      }
      case 'image': {
        let f = o.frame;
        if (!f) {
          const { W, H } = this.slideSize();
          let w = (o.sizeW ?? 640) * EMU_PER_PX, h = (o.sizeH ?? 480) * EMU_PER_PX;
          const k = Math.min(1, (W * 0.8) / w, (H * 0.8) / h);
          w *= k; h *= k;
          f = { x: (W - w) / 2, y: (H - h) / 2, cx: w, cy: h };
        }
        return this.insertWith('image', (pkg, p) => deckEdit.insertImage(pkg, p, o.bytes, o.mime, f).id);
      }
      case 'frame': {
        if (this.inGroup(o.shape)) return undefined;
        return onShape('frame', (pkg, p) => deckEdit.setShapeFrame(pkg, p, o.shape, { x: o.x, y: o.y, cx: o.cx, cy: o.cy }));
      }
      case 'fill': return onShape('fill', (pkg, p) => deckEdit.setShapeFill(pkg, p, o.shape, o.color));
      case 'outline': return onShape('outline', (pkg, p) => deckEdit.setShapeOutline(pkg, p, o.shape, o.color));
      case 'arrange': return onShape('arrange', (pkg, p) => deckEdit.arrangeShape(pkg, p, o.shape, o.mode));
      case 'deleteShape': { const p = this.slidePath(); return this.edit('deleteShape', (pkg) => deckEdit.deleteShape(pkg, p, o.shape), 'slide', { select: null }); }
      case 'copy': {
        if (this.inGroup(o.shape)) { this.clip = null; return false; }
        this.clip = deckEdit.extractShape(this.pkg, this.slidePath(), o.shape);
        return this.clip ? true : false;
      }
      case 'xpaste': {
        // a shape copied from ANOTHER package (the bytes of its images travel with the clip), then pasted here
        const src = xsession(o.deck);
        const p = src.paths()[o.slide];
        if (!p) return undefined;
        const clip = deckEdit.extractShape(src.pkg, p, o.shape);
        if (!clip) return undefined;
        this.clip = clip;
        return this.exec({ op: 'paste' });
      }
      case 'paste': {
        if (!this.clip) return undefined;
        if (!this.slideCount()) this.run({ op: 'addSlide', layout: undefined });
        const clip = this.clip;
        const path = this.slidePath();
        const sameSlide = clip.source === this.pkg && clip.slidePath === this.paths()[this.index];
        const d = sameSlide ? 12700 * 12 : 0;
        const id = this.edit('paste', (pkg) => deckEdit.pasteShape(pkg, path, clip, d, d) ?? '', 'slide', { select: null });
        if (!id) return 's';
        this.selectById(id);
        return `s${id}`;
      }
      case 'notes': {
        if (!this.hasSlide(o.i)) return false;
        const p = this.slidePath(o.i);
        if (core.readSlideNotes(this.pkg, p) === o.text) return false;
        return !!this.edit('notes', (pkg) => deckEdit.setSlideNotes(pkg, p, o.text, lang), o.i);
      }
      case 'properties': {
        const p = {};
        for (const k of ['title', 'subject', 'author', 'keywords', 'description', 'lastModifiedBy']) if (o[k] !== undefined) p[k] = o[k];
        this.edit('properties', (pkg) => deckEdit.writeProperties(pkg, p), 'deck');
        return undefined;
      }
      case 'replace': {
        const hits = o.hit ? [o.hit] : this.findText(o.query, o.matchCase);
        if (!hits.length) return 0;
        const paths = hits.map((h) => this.slidePath(h.slide));
        let count = 0;
        this.edit('replace', (pkg) => { hits.forEach((h, k) => { count += deckEdit.replaceInShape(pkg, paths[k], h.shape, o.query, o.replacement, o.matchCase); }); }, 'deck');
        return count;
      }
      case 'select': this.selected = null; this.selectById(o.shape); return undefined;
      case 'goto': if (o.i !== this.index) { this.index = o.i; this.selected = null; } return undefined;
      case 'undo': return this.undo();
      case 'redo': return this.redo();
      case 'markClean': this.history.markSaved(); this.embeddingTouched = false; return undefined;
      case 'embed':
        if (o.embed !== undefined) this.embedding.embed = o.embed;
        if (o.subset !== undefined) this.embedding.subset = o.subset;
        this.embeddingTouched = true;
        return undefined;
      default: throw new Error(`unknown op ${o.op}`);
    }
  }

  stateFacet() {
    const h = this.history;
    return `i=${this.index};sel=${this.selected ?? '-'};u=${+h.canUndo()};r=${+h.canRedo()};ul=${h.undoLabel() ?? '-'};rl=${h.redoLabel() ?? '-'};d=${+(h.dirty || this.embeddingTouched)}`;
  }
}

// ───────────────────────── fonts in the file ─────────────────────────

const FONT_FILES = join(root, 'shared', 'fonts', 'files');
const FACE_FILES = { caladea: 'caladea', 'liberation serif': 'liberationserif', 'liberation mono': 'liberationmono', carlito: 'carlito' };
const VARIANT_FILE = { regular: 'regular', bold: 'bold', italic: 'italic', boldItalic: 'bolditalic' };
const faceBytes = (family, variant) => {
  const base = FACE_FILES[family.toLowerCase()];
  return base ? new Uint8Array(readFileSync(join(FONT_FILES, `${base}-${VARIANT_FILE[variant]}.ttf`))) : undefined;
};
const stubResolver = {
  resolve: async (families) => families.map((family) => ({ family, source: faceBytes(family, 'regular') ? 'host' : 'substitute', faces: {} })),
  loadFace: async (r, v) => faceBytes(r.family, v),
};

/** `collectDeckTypefaces` and `deckFontUsage` of universal-slides.ts (that function is private there). */
function deckFontUsage(pkg) {
  const collect = (p) => {
    const out = new Set();
    for (const part of p.partNames()) {
      if (!/^ppt\/(slides|slideLayouts|slideMasters)\/[^/]+\.xml$/.test(part)) continue;
      const xml = p.getPartText(part);
      if (!xml) continue;
      for (const m of xml.matchAll(/typeface="([^"+][^"]*)"/g)) out.add(m[1]);
    }
    return [...out].sort();
  };
  const ent = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'" };
  const unesc = (s) => s.replace(/&(#x[0-9a-f]+|#\d+|amp|lt|gt|quot|apos);/gi, (_, e) => (e[0] === '#' ? String.fromCodePoint(e[1] === 'x' || e[1] === 'X' ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10)) : ent[e.toLowerCase()]));
  const families = new Set(collect(pkg));
  let text = '';
  for (const part of pkg.partNames()) {
    const xml = /^ppt\/(slides|slideLayouts|slideMasters|theme)\/[^/]+\.xml$/.test(part) ? pkg.getPartText(part) : undefined;
    if (!xml) continue;
    if (part.startsWith('ppt/theme/')) {
      for (const m of xml.matchAll(/<a:(?:major|minor)Font>\s*<a:latin\b[^>]*typeface="([^"]+)"/g)) families.add(unesc(m[1]));
      continue;
    }
    for (const m of xml.matchAll(/<a:t>([^<]*)<\/a:t>/g)) text += unesc(m[1]);
  }
  return [...families].filter((f) => f && !f.startsWith('+')).map((family) => ({ family, variants: ['regular'], text }));
}

/** `savePptx` of universal-slides.ts with the stub resolver: the parts of the saved package. */
async function saveFontsParts(sess) {
  const pkg = core.PptxPackage.load(sess.pkg.save());
  const settings = sess.embedding;
  let plan = { faces: [], skipped: [] };
  if (settings.embed) plan = await fontsEmbedding.planFontEmbedding(stubResolver, deckFontUsage(pkg), settings, 'slides', { alreadyEmbedded: fontsOoxml.readPptxEmbedding(pkg).embedded });
  fontsOoxml.writePptxEmbedding(pkg, plan, settings);
  return core.PptxPackage.load(pkg.save());
}

// ───────────────────────── the script generator ─────────────────────────

const TEXTS = ['Hello', 'Olá, mundo! ção', 'Line one\nLine two', 'A&B <C> "D"', '𝒜 😀 emoji', '', 'x', 'Lorem ipsum dolor sit amet', 'One\n\nThree', 'tab\there $& $1'];
const COLORS = ['FF0000', '#00aa00', '0000ff', 'ffcc00', '#abc', 'ff', '', '123456789'];
const FONTS = ['Caladea', 'Liberation Serif', 'Liberation Mono', 'Carlito', 'Arial', 'Calibri'];
const PRSTS = ['rect', 'roundRect', 'ellipse', 'triangle', 'rightArrow', 'star5', 'heart', 'line', 'cloud', 'smileyFace'];
const ALIGNS = ['l', 'ctr', 'r', 'just'];
const BULLETS = ['bullet', 'number', 'none'];
const MODES = ['front', 'back', 'forward', 'backward'];
const TRANS = ['fade', 'push', 'wipe', 'split', 'cover', 'pull', 'randomBar', 'circle', 'diamond', 'plus', 'wedge', 'wheel', 'zoom', 'dissolve', 'newsflash', 'blinds', 'checker', 'strips', 'comb', 'cut', 'none'];
const URLS = ['https://example.com/a?b=1&c=2', 'http://localhost:8080/x', 'mailto:me@example.org'];
// a tiny valid PNG (1x1) and an arbitrary payload
const PNG = Uint8Array.from(Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64'));

function genScript(sess, rng, steps, mix) {
  const pick = (arr) => arr[rng.below(arr.length)];
  const num = (lo, hi) => lo + rng.below(hi - lo + 1);
  const frac = (lo, hi) => lo + rng.below((hi - lo) * 10 + 1) / 10;
  const out = [];
  const shapesOf = (i = sess.index) => (sess.slides()[i]?.shapes ?? []).filter((s) => !s.inherited);
  const textShape = () => { const t = shapesOf().filter((s) => s.txBody); return t.length ? pick(t) : null; };
  const anyShape = () => { const t = shapesOf(); return t.length ? pick(t) : null; };
  const frame = () => ({ x: frac(0, 9e6), y: frac(0, 5e6), cx: frac(1e5, 5e6), cy: frac(1e5, 3e6) });
  const table = [
    [3, () => { const s = textShape(); return s && { op: 'text', shape: s.id, text: pick(TEXTS) }; }],
    [2, () => { const s = textShape(); return s && { op: 'typing', slide: sess.index, shape: s.id, text: pick(TEXTS) }; }],
    [3, () => { const s = textShape(); return s && { op: 'format', shape: s.id, ...pick([{ bold: true }, { italic: false }, { underline: true, strike: false }, { size: frac(1, 80) }, { font: pick(FONTS) }, { color: pick(COLORS) }, { bold: false, size: num(8, 44), color: pick(COLORS), font: pick(FONTS) }, { size: 0.2 }, { size: 9999 }]) }; }],
    [1, () => { const s = textShape(); return s && { op: 'grow', shape: s.id, dir: pick([1, -1]), base: pick([18, 11, 96, 8]) }; }],
    [1, () => { const s = textShape(); return s && { op: 'align', shape: s.id, algn: pick(ALIGNS) }; }],
    [2, () => { const s = textShape(); return s && { op: 'bullets', shape: s.id, kind: pick(BULLETS) }; }],
    [1, () => { const s = textShape(); return s && { op: 'clearFormat', shape: s.id }; }],
    [1, () => { const s = textShape(); return s && { op: 'link', shape: s.id, url: rng.below(3) ? pick(URLS) : null }; }],
    [3, () => ({ op: 'textBox', text: pick(TEXTS), frame: rng.below(2) ? frame() : undefined })],
    [3, () => ({ op: 'shape', prst: pick(PRSTS), frame: rng.below(2) ? frame() : undefined })],
    [1, () => ({ op: 'image', bytes: rng.below(2) ? PNG : Uint8Array.from([1, 2, 3, rng.below(256)]), mime: pick(['image/png', 'image/jpeg', 'image/gif', 'image/svg+xml', 'image/x-unknown']), frame: rng.below(2) ? frame() : undefined, sizeW: rng.below(2) ? num(10, 4000) : undefined, sizeH: rng.below(2) ? num(10, 3000) : undefined })],
    [3, () => { const s = anyShape(); return s && { op: 'frame', shape: s.id, ...frame() }; }],
    [2, () => { const s = anyShape(); return s && { op: 'fill', shape: s.id, color: rng.below(4) ? pick(COLORS) : null }; }],
    [2, () => { const s = anyShape(); return s && { op: 'outline', shape: s.id, color: rng.below(4) ? pick(COLORS) : null }; }],
    [2, () => { const s = anyShape(); return s && { op: 'arrange', shape: s.id, mode: pick(MODES) }; }],
    [2, () => { const s = anyShape(); return s && { op: 'deleteShape', shape: s.id }; }],
    [1, () => { const s = anyShape(); return s && { op: 'copy', shape: s.id }; }],
    [2, () => (sess.clip ? { op: 'paste' } : null)],
    [2, () => {
      const deck = pick(XDECKS);
      const x = xsession(deck);
      const slide = rng.below(Math.max(1, x.slideCount()));
      const sh = (x.slides()[slide]?.shapes ?? []).filter((q) => !q.inherited);
      return sh.length ? { op: 'xpaste', deck, slide, shape: pick(sh).id } : null;
    }],
    [2, () => ({ op: 'addSlide', layout: rng.below(2) ? pick(deckEdit.listLayouts(sess.pkg).map((l) => l.path).concat([undefined])) : undefined })],
    [2, () => (sess.slideCount() ? { op: 'duplicateSlide', i: rng.below(sess.slideCount()) } : null)],
    [1, () => (sess.slideCount() >= 1 ? { op: 'deleteSlide', i: rng.below(sess.slideCount()) } : null)],
    [2, () => (sess.slideCount() > 1 ? { op: 'moveSlide', from: rng.below(sess.slideCount()), to: rng.below(sess.slideCount()) } : null)],
    [1, () => (sess.slideCount() ? { op: 'hideSlide', i: rng.below(sess.slideCount()), hidden: !!rng.below(2) } : null)],
    [1, () => { const l = deckEdit.listLayouts(sess.pkg); return sess.slideCount() && l.length ? { op: 'layout', i: rng.below(sess.slideCount()), layout: pick(l).path } : null; }],
    [1, () => (sess.slideCount() ? { op: 'background', target: rng.below(4) ? rng.below(sess.slideCount()) : 'all', color: rng.below(4) ? pick(COLORS) : null } : null)],
    [1, () => (sess.slideCount() ? { op: 'transition', target: rng.below(4) ? rng.below(sess.slideCount()) : 'all', spec: rng.below(5) ? { type: pick(TRANS), dir: pick(['l', 'r', 'u', 'd', 'in', 'out', undefined]), speed: pick(['fast', 'med', 'slow', undefined]), advanceOnClick: pick([true, false, undefined]), advanceAfterMs: pick([undefined, 0, 2500, 1234.5, -1]) } : null } : null)],
    [1, () => ({ op: 'showSettings', ...pick([{ loop: true }, { useTimings: false }, { showAnimation: false, kiosk: true }, { kiosk: false }, { loop: false, useTimings: true, showAnimation: true }]) })],
    [2, () => (sess.slideCount() ? { op: 'notes', i: rng.below(sess.slideCount()), text: pick(TEXTS) } : null)],
    [1, () => ({ op: 'properties', ...pick([{ title: 'T & <1>' }, { author: 'Eric', subject: 'S' }, { keywords: 'a,b', description: 'd\nd' }, { title: '' }, { lastModifiedBy: 'me' }]) })],
    [2, () => { const s = textShape(); if (!s) return null; const t = s.txBody.paragraphs.map((p) => p.runs.map((r) => r.text).join('')).join(' '); const q = t.length > 2 ? t.slice(0, Math.max(1, Math.min(3, t.length))) : 'e'; return { op: 'replace', query: q, replacement: pick(['X', '', '$&', 'ção']), matchCase: !!rng.below(2) }; }],
    [3, () => ({ op: 'undo' })],
    [2, () => ({ op: 'redo' })],
    [1, () => ({ op: 'markClean' })],
    [2, () => { const sh = anyShape(); return { op: 'select', shape: sh && rng.below(4) ? sh.id : null }; }],
    [2, () => (sess.slideCount() ? { op: 'goto', i: rng.below(sess.slideCount()) } : null)],
  ];
  const total = table.reduce((s, [w]) => s + w, 0);
  for (let k = 0; k < steps; k++) {
    let op = null;
    for (let tries = 0; tries < 20 && !op; tries++) {
      let r = rng.below(total);
      for (const [w, f] of table) { if (r < w) { op = f(); break; } r -= w; }
    }
    if (!op) op = { op: 'undo' };
    if (mix === 'fonts' && k % 9 === 3) op = { op: 'embed', embed: !!rng.below(3), subset: !!rng.below(2) };
    if (mix === 'fonts' && k % 9 === 4) op = { op: 'saveFonts' };
    out.push(op);
    sess.exec2(op);
  }
  return out;
}

// the generator needs to advance the session as it goes (the script depends on the state): `exec2` = run, ignoring `ret`
Session.prototype.exec2 = function exec2(op) { try { this.run(op); } catch { /* the capture pass records the error */ } };

// ───────────────────────── capture ─────────────────────────

const writer = new CaptureWriter([
  'Slides editing session (B8): the frozen TS deck-edit operations under the edit()/undo()/redo() protocol of universal-slides.ts, driven by seeded scripts; see js/capture-edit.mjs and core/docs/modules/uo-pptx.md "B8"',
]);
writer.meta('ts-root-commit', tsCommit(root));
writer.meta('random', 'Math.random = xorshift32 seed 1 at the start of every script; Date fixed 2026-01-02T03:04:00Z');
const opLines = [];
const partLines = [];
const stats = { scripts: 0, steps: 0, errors: 0 };
const dumpKey = a.opt('--dump');

/** Runs one script (op list given, or generated) over a session; records every step. */
async function record(key, bytes, { steps, seed, mix, ops, lang, blank }) {
  reseed();
  let gen;
  try {
    gen = new Session(bytes, lang);
    gen.history.clear();
  } catch (e) { return false; }
  if (!ops) {
    // generation pass on a scratch session (it consumes Math.random exactly as the capture pass will not: reseeded below)
    ops = genScript(gen, new Rng(seed), steps, mix);
  }
  reseed();
  const sess = new Session(bytes, lang);
  let prev = partMap(sess.pkg);
  const emit = (k, op, ret) => {
    writer.set(`${key}#${k}`, 'op', op);
    writer.set(`${key}#${k}`, 'ret', ret);
    writer.set(`${key}#${k}`, 'parts', partsDigest(sess.pkg));
    writer.set(`${key}#${k}`, 'st', sess.stateFacet());
    const c = curRecord(sess.pkg, sess.index);
    writer.set(`${key}#${k}`, 'pres', c.pres);
    writer.set(`${key}#${k}`, 'cur', c.cur);
    const now = partMap(sess.pkg);
    for (const [n, d] of now) if (prev.get(n) !== d) partLines.push(`${key}#${k}\t${n}\t${d}`);
    for (const n of prev.keys()) if (!now.has(n)) partLines.push(`${key}#${k}\t${n}\t-`);
    prev = now;
  };
  opLines.push(`${key}#0\tinit\t${esc(lang ?? 'en-US')}\t${tok(blank ? blank.aspect !== '4:3' : undefined)}\t${tok(blank?.title)}`);
  emit(0, 'init', 'none');
  for (let k = 0; k < ops.length; k++) {
    const op = ops[k];
    let ret;
    try {
      if (op.op === 'saveFonts') {
        const out = await saveFontsParts(sess);
        ret = `s${partsDigest(out)}`;
      } else ret = sess.run(op);
    } catch (e) { ret = 'err'; stats.errors++; if (a.flag('--verbose')) console.error(`${key}#${k + 1} ${op.op}: ${e?.message ?? e}`); }
    opLines.push(`${key}#${k + 1}\t${ser(op).join('\t')}`);
    emit(k + 1, op.op, ret);
    if (dumpKey === `${key}#${k + 1}`) console.log([...sess.pkg.partNames()].map((n) => `${n}: ${partMap(sess.pkg).get(n)}`).join('\n'));
    stats.steps++;
  }
  stats.scripts++;
  return true;
}

const only = a.opt('--only');
const steps = Number(a.opt('--steps', '60'));
let mode;
if (a.flag('--samples')) {
  mode = 'samples';
  for (const rel of trackedSamples()) {
    if (!/\.pptx$/i.test(rel)) continue;
    const key = `samples/${rel}`;
    if (only && !key.includes(only)) continue;
    await record(key, readSample(rel), { steps, seed: 1 + [...rel].reduce((s, c) => s + c.charCodeAt(0), 0), mix: 'plain' });
  }
} else if (a.opt('--gen')) {
  const names = a.opt('--gen').split(',');
  mode = names.join('+');
  const decksDir = a.opt('--decks', join(capturesDir, 'decks'));
  const n = Number(a.opt('--n', '40'));
  for (const name of names) {
    const g = GROUPS[name];
    if (!g) throw new Error(`unknown group ${name}`);
    for (let i = 0; i < Math.min(n, g.count); i++) {
      const key = groupKey(name, i);
      if (only && !key.includes(only)) continue;
      await record(key, new Uint8Array(readFileSync(join(decksDir, name, deckFile(name, i)))), { steps, seed: g.seedBase + i, mix: 'plain' });
    }
  }
} else if (a.flag('--blank')) {
  mode = 'blank';
  // the blank deck itself: byte-equal parts (every variant), then long scripts from it
  const variants = [{}, { aspect: '4:3' }, { title: 'Quarterly <review> & plan', lang: 'pt-BR' }, { aspect: '4:3', title: 'x', lang: 'es-ES' }];
  variants.forEach((v, i) => {
    const key = `blank/${i}`;
    const pkg = core.PptxPackage.load(blankDeck(v));
    writer.set(key, 'parts', partsDigest(pkg));
    for (const n of pkg.partNames()) writer.set(key, `p:${n}`, bytesDigest(pkg.getPart(n)));
  });
  for (let i = 0; i < 12; i++) {
    const v = variants[i % variants.length];
    await record(`blank/s${i}`, blankDeck(v), { steps: Number(a.opt('--steps', '120')), seed: 4000 + i, mix: 'plain', lang: v.lang ?? 'en-US', blank: v });
  }
} else if (a.flag('--fonts')) {
  mode = 'fonts';
  const decksDir = a.opt('--decks', join(capturesDir, 'decks'));
  for (let i = 0; i < 24; i++) {
    const key = groupKey('b1a', i);
    await record(key, new Uint8Array(readFileSync(join(decksDir, 'b1a', deckFile('b1a', i)))), { steps: 54, seed: 9000 + i, mix: 'fonts' });
  }
  await record('blank/f0', blankDeck({}), { steps: 80, seed: 9100, mix: 'fonts', blank: {} });
} else {
  console.error('usage: capture-edit.mjs (--samples | --gen <group[,group]> | --blank | --fonts) [--out stem] [--steps n] [--n decks]');
  process.exit(2);
}

writer.meta('scripts', stats.scripts);
writer.meta('steps', stats.steps);
const stem = a.opt('--out', join(capturesDir, `edit-${mode}`));
mkdirSync(dirname(stem), { recursive: true });
writer.write(`${stem}.capture`);
// gzip header carries no mtime (Node writes 0)
writeFileSync(`${stem}.ops.gz`, gzipSync(Buffer.from(`${opLines.join('\n')}\n`, 'utf8'), { level: 9 }));
writeFileSync(`${stem}.parts.gz`, gzipSync(Buffer.from(`${partLines.join('\n')}\n`, 'utf8'), { level: 9 }));
console.log(`${mode}: ${stats.scripts} scripts, ${stats.steps} steps, ${stats.errors} TS errors -> ${stem}.capture`);
