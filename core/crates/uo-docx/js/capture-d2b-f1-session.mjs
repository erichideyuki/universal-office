// The D2b-F1 editor harness (character and paragraph formatting; record uo-docx.md "D2a — session
// (architecture)", "D2b frontier briefs", docs/modules/uo-docx-d2b/F1.md): the frozen
// `<universal-write>` element in headless Chromium (Playwright), driven through the public methods
// of the F1 commands mixed with the D2a ops (typing, Enter, Backspace, Delete, selection, tracking,
// undo, redo), one element per page load, `Math.random` replaced by the testkit xorshift32 and
// `Date` fixed (2026-01-02 03:04 UTC, timezone UTC) as in the D2a harness (which this file copies:
// that script is shared and not imported; its top level runs the capture).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f1-session.mjs [--docs N] [--steps N]
//
// Writes tests/parity/d2b-f1-session.capture: per `<doc>#<j>` the facets of the D2a harness (op,
// paras, sel, anc, ev, flags, docxml) plus `pend`, the JSON of `el.pendingProps` (the props of the
// next typed text; symbol keys do not show). Declared normalisation: the horizontal-line shape the
// editor builds (`{ style: { fill } }`) has no `geom`; the Rust style requires one (`rect`), so the
// TS side gets `geom: 'rect'` in the compared state.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { join, extname, normalize } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter, Rng, Stream } from '../../uo-testkit/js/testkit.mjs';
const Digest = { of: (b) => ({ short: createHash('sha256').update(b).digest('hex').slice(0, 32) }) };
import { Canon } from './canon.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 96);
const STEPS = opt('--steps', 80);
// `--dump <doc>#<step>:<para>`: prints the JSON of that paragraph of the restored state and stops
const DUMP = argv.includes('--dump') ? argv[argv.indexOf('--dump') + 1] : undefined;
const { DocxPackage } = await ts('package.js');

const pwFrom = [join(root, 'slides', 'package.json'), join(root, 'sheet', 'package.json')].find((p) => existsSync(p));
const { chromium } = createRequire(pwFrom)('playwright');

const MIME = { '.js': 'text/javascript', '.mjs': 'text/javascript', '.html': 'text/html', '.json': 'application/json', '.wasm': 'application/wasm', '.css': 'text/css', '.ttf': 'font/ttf', '.otf': 'font/otf', '.woff2': 'font/woff2' };
const server = createServer((req, res) => {
  const url = decodeURIComponent((req.url ?? '/').split('?')[0]);
  if (url === '/__d2a.html') {
    res.writeHead(200, { 'content-type': 'text/html' });
    res.end('<!doctype html><meta charset="utf-8"><body><script type="module">import "/write/packages/editor/publish/index.js"; window.__ready = true;</script></body>');
    return;
  }
  const file = normalize(join(root, url));
  if (!file.startsWith(normalize(root)) || !existsSync(file)) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': MIME[extname(file)] ?? 'application/octet-stream' });
  res.end(readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

// the Chromium build installed with Playwright; `UO_CHROMIUM` overrides (a Playwright newer than
// the installed browsers looks for a build that is not there)
const installed = (() => {
  const base = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (!existsSync(base)) return undefined;
  const dirs = readdirSync(base).filter((d) => d.startsWith('chromium_headless_shell-')).sort();
  const last = dirs[dirs.length - 1];
  const exe = last && join(base, last, 'chrome-headless-shell-win64', 'chrome-headless-shell.exe');
  return exe && existsSync(exe) ? exe : undefined;
})();
const browser = await chromium.launch({ headless: true, executablePath: process.env.UO_CHROMIUM ?? installed });
const context = await browser.newContext({ timezoneId: 'UTC', locale: 'pt-BR' });
await context.addInitScript(() => {
  // the testkit xorshift32 (seed 1), as `FixedEnv::default()`
  let s = 1;
  Math.random = () => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s / 4294967296; };
  const T = Date.UTC(2026, 0, 2, 3, 4, 0);
  const D = Date;
  // eslint-disable-next-line no-global-assign
  Date = class extends D { constructor(...a) { if (a.length) super(...a); else super(T); } static now() { return T; } };
});

const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-input', 'uw-track-changes', 'uw-cellmode', 'uw-notemode', 'uw-hfmode', 'uw-tbmode', 'uw-protected', 'uw-blocked'];
const TEXTS = ['a', 'Olá ', ' ', 'ção', '😀', '𝒜!', 'abc def', 'ABC. déF! ǆ ß'];
const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');

/** Uint8Arrays across the page boundary. */
const revive = (v) => (v && typeof v === 'object' ? (v.__u8 !== undefined ? Uint8Array.from(Buffer.from(v.__u8, 'base64')) : (Array.isArray(v) ? v.map(revive) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, revive(x)])))) : v);

function digest(paras) {
  const s = new Stream();
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  return s.finish().short;
}

const out = new CaptureWriter([
  'D2b-F1 session in Chromium: the frozen <universal-write> driven by seeded scripts (character and paragraph formatting mixed with typing, Enter, Backspace, Delete, selection, undo, redo, tracking)',
  'facets: op, paras, sel, anc, ev, flags, pend, docxml',
]);
out.meta('steps', STEPS);
out.meta('browser', `chromium ${browser.version()}`);
out.meta('bundle', Digest.of(readFileSync(join(root, 'write/packages/editor/publish/index.js'))).short);
out.meta('math-override', 'none (no F1 state value goes through math)');

// the argument tables of the F1 ops (the Rust replay has the same tables, by index)
const T = {
  TOGGLES: ['bold', 'italic', 'underline', 'strike', 'doubleStrike', 'allCaps', 'smallCaps', 'hidden'],
  UL: ['none', 'single', 'double', 'thick', 'dotted', 'wave', 'words'],
  CASES: ['upper', 'lower', 'sentence', 'title', 'toggle', 'next'],
  // hex, or `name[:t<tint>|:s<shade>]` for a theme colour, or null
  COLORS: ['ff0000', '#00aa00', 'accent1', 'accent2:t0.4', 'text1:s0.25', 'background1:t0.8', 'dark2', null],
  FX: [
    { fill: { type: 'solid', color: { rgb: 'FF0000' } } },
    { outline: null },
    { shadow: { blurRad: 50800, dist: 38100, dir: 2700000, sx: 100000, sy: 100000, kx: 0, ky: 0, algn: 'tl', color: { rgb: '000000', mods: [['alpha', 60000]] } } },
    { glow: { rad: 63500, color: { scheme: 'accent1', mods: [['alpha', 60000]] } }, fill: null },
    { props3d: '<w14:props3d/>', scene3d: null },
    { fill: { type: 'solid', color: { scheme: 'accent2' } }, outline: { w: 9525, cap: 'flat', cmpd: 'sng', algn: 'ctr', fill: { type: 'solid', color: { rgb: '00FF00' } }, dash: 'solid', join: 'round' } },
  ],
  OT: [{ ligatures: 'standard' }, { numForm: 'oldStyle', numSpacing: 'tabular' }, { ligatures: null, numForm: 'default' }, { stylisticSets: [1, 3], cntxtAlts: true }, { stylisticSets: [], cntxtAlts: false }],
  GRAD: [null, { angleDeg: 90, light: true }, { angleDeg: 45.5, light: false }],
  FF: [
    { fontFamily: 'Arial', sizePt: 12.5, bold: true },
    { underline: 'double', underlineColor: '00FF00', color: { themeColor: 'accent1', tint: 0.4 } },
    { strike: true, superscript: true, scalePct: 100, spacingTwips: 0 },
    { italic: false, subscript: true, scalePct: 150, spacingTwips: 20, positionHalfPt: 4, kernMinHalfPt: 0 },
    { underline: 'none', color: null, underlineColor: null, smallCaps: true, allCaps: false, hidden: true },
    {},
    { doubleStrike: true, underline: 'single' },
  ],
  TABS: [
    { tabs: [] },
    { tabs: [{ pos: 720, align: 'left' }, { pos: 3600, align: 'right', leader: 'dot' }] },
    { tabs: [{ pos: 1440, align: 'center' }, { pos: 1440, align: 'decimal' }] },
    { tabs: [{ pos: 2000.4, align: 'bar' }] },
    { tabs: [], only: { add: [{ pos: 1000, align: 'left' }], remove: [720] } },
    { tabs: [], only: { remove: [1440.2] } },
  ],
  FLOW: [{ keepNext: true }, { widowControl: false, keepLines: true }, { pageBreakBefore: true, suppressLineNumbers: true, suppressAutoHyphens: true }, { textboxTightWrap: 'allLines' }, { widowControl: true, keepNext: false }, {}],
  LAYOUT: [{ left: 720 }, { right: 360, before: 'auto' }, { before: 240, after: 120 }, { after: 'auto', left: -100000 }, { before: 0 }, { left: 0, right: 0 }, { before: 12.6 }],
  BORDERS: [null, { top: { val: 'single', sz: 8, space: 2, color: 'FF0000' }, bottom: { val: 'double' } }, { left: { val: 'single' }, top: null }, { between: { val: 'nil' }, bar: { val: 'single', color: 'auto' } }, {}],
  INDENTS: [{ left: 720 }, { firstLine: -360, right: 200 }, { left: 0, right: 0, firstLine: 0 }, { firstLine: 360 }, { left: 1000.5 }],
  PRESETS: ['bottom', 'top', 'left', 'right', 'none', 'all', 'outside', 'inside', 'insideH', 'insideV'],
  ALIGN: ['left', 'center', 'right', 'both'],
  FAMILIES: ['Arial', 'Calibri', 'Liberation Sans'],
  HL: ['yellow', 'green', 'red', '-'],
  SIZES: [8, 10.5, 11.25, 14, 22.5, 72.4],
};

const F1_OPS = [
  (r) => `tg:${r.below(T.TOGGLES.length)}`,
  (r) => `tu:${r.below(2) ? '-' : T.UL[1 + r.below(T.UL.length - 1)]}`,
  (r) => `su:${r.below(T.UL.length)}`,
  (r) => `uc:${r.below(T.COLORS.length)}`,
  (r) => `fx:${r.below(T.FX.length)}`,
  (r) => `fp:${r.below(19) - 1}`,
  (r) => `ot:${r.below(T.OT.length)}`,
  (r) => `tgr:${r.below(T.GRAD.length)}`,
  (r) => `ff:${r.below(T.FF.length)}`,
  (r) => `va:${r.below(2)}`,
  (r) => `fs:${r.below(T.SIZES.length)}`,
  (r) => `gf:${[-2, -0.5, 0, 1, 3][r.below(5)]}`,
  (r) => `fam:${r.below(T.FAMILIES.length)}`,
  (r) => `co:${r.below(T.COLORS.length)}`,
  (r) => `hl:${r.below(T.HL.length)}`,
  () => 'cf',
  (r) => `cc:${r.below(T.CASES.length)}`,
  (r) => `cs:${[-20, 0, 15, '-'][r.below(4)]}`,
  () => 'ss',
  (r) => `al:${r.below(T.ALIGN.length)}`,
  (r) => `in:${r.below(2) ? 1 : -1}`,
  (r) => `ls:${[240, 276, 360, 480][r.below(4)]}`,
  (r) => `sa:${[0, 120, 200, '-'][r.below(4)]}`,
  (r) => `sb:${[0, 120, 200, '-'][r.below(4)]}`,
  (r) => `sh:${['ffff00', 'cccccc', '-'][r.below(3)]}`,
  (r) => `si:${r.below(3)}`,
  (r) => `ir:${[0, 360, 720, '-'][r.below(4)]}`,
  (r) => `pi:${r.below(T.INDENTS.length)}`,
  (r) => `ts:${r.below(T.TABS.length)}`,
  (r) => `pf:${r.below(T.FLOW.length)}`,
  (r) => `pl:${r.below(T.LAYOUT.length)}`,
  (r) => `pb:${r.below(T.BORDERS.length)}`,
  (r) => `bp:${r.below(T.PRESETS.length)}`,
  () => 'tb',
  () => 'hr',
  () => 'cp',
];

const files = corpus().filter((_, i) => i % 23 === 7).slice(0, DOCS).filter((f) => !DUMP || DUMP.startsWith(`${f.rel}#`));
let n = 0;
for (const { rel, file } of files) {
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${port}/__d2a.html`);
  await page.waitForFunction(() => window.__ready === true);
  const bytes = readBytes(file);
  const ok = await page.evaluate(async ([b64, EVENTS]) => {
    const el = document.createElement('universal-write');
    el.setAttribute('fonts', '/write/packages/editor/demo/fonts');
    const ready = new Promise((r) => el.addEventListener('uw-ready', r, { once: true }));
    document.body.append(el);
    await ready;
    window.__el = el;
    window.__ev = [];
    for (const t of EVENTS) el.addEventListener(t, (e) => { const d = e.detail ?? {}; window.__ev.push(`${t}:${d.dirty ?? d.active ?? d.on ?? ''}`); });
    try {
      el.author = 'A';
      el.loadDocx(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)), { protected: false });
      return !!el.fonts;
    } catch { return false; }
  }, [Buffer.from(bytes).toString('base64'), EVENTS]);
  if (!ok) { await page.close(); continue; }
  let seed = 0x5e55 ^ 0xf1f1 ^ 0x2222;
  for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
  const rng = new Rng(seed);
  const lens = () => page.evaluate(() => window.__el.model.paragraphs.map((p) => p.runs.reduce((a, r) => a + r.text.length, 0)));
  // a paragraph of the selection is numbered (the TS `indent` goes to the F2 list levels)
  const numbered = () => page.evaluate(() => {
    const el = window.__el;
    const a = el.selection.anchor.para, f = el.selection.focus.para;
    for (let i = Math.min(a, f); i <= Math.max(a, f); i++) if (el.paraNumId(el.model.paragraphs[i]) !== 0) return true;
    return false;
  });
  for (let j = 0; j <= STEPS; j++) {
    const key = `${rel}#${j}`;
    let code = 'init';
    if (j > 0) {
      const L = await lens();
      const pos = () => { const para = rng.below(L.length); return { para, offset: rng.below(L[para] + 1) }; };
      const k = rng.below(100);
      if (k < 8) code = `ty:${hex(TEXTS[rng.below(TEXTS.length)])}`;
      else if (k < 11) code = 'en';
      else if (k < 14) code = 'bs';
      else if (k < 16) code = 'de';
      else if (k < 36) { const a = pos(); const f = rng.below(4) === 0 ? a : pos(); code = `se:${a.para}:${a.offset}:${f.para}:${f.offset}`; }
      else if (k < 38) code = `tk:${rng.below(2)}`;
      else if (k < 43) code = 'un';
      else if (k < 46) code = 're';
      else if (k < 49) { const a = pos(); const s = pos(); const e = pos(); code = `pt:${a.para}:${a.offset}:${s.para}:${s.offset}:${e.para}:${e.offset}`; }
      else code = F1_OPS[rng.below(F1_OPS.length)](rng);
      // the numbered paragraphs go to F2's list levels (changeListLevel): not generated
      if (code.startsWith('in:') && await numbered()) code = 'cp';
      await page.evaluate(([code, T]) => {
        const el = window.__el;
        window.__ev = [];
        const f = code.split(':');
        const colorOf = (i) => {
          const c = T.COLORS[+i];
          if (c === null) return null;
          if (/^#?[0-9a-f]{6}$/.test(c)) return c;
          const [name, m] = c.split(':');
          return { themeColor: name, ...(m ? (m[0] === 't' ? { tint: Number(m.slice(1)) } : { shade: Number(m.slice(1)) }) : {}) };
        };
        const clone = (x) => JSON.parse(JSON.stringify(x));
        switch (f[0]) {
          case 'ty': el.type(String.fromCharCode(...f[1].split('.').map((h) => parseInt(h, 16)))); break;
          case 'en': el.pressEnter(); break;
          case 'bs': el.pressBackspace(); break;
          case 'de': el.pressDelete(); break;
          case 'se': el.setSelection({ para: +f[1], offset: +f[2] }, { para: +f[3], offset: +f[4] }); break;
          case 'tk': el.toggleTrackChanges(f[1] === '1'); break;
          case 'un': el.undo(); break;
          case 're': el.redo(); break;
          case 'pt': {
            el.setSelection({ para: +f[1], offset: +f[2] }, { para: +f[1], offset: +f[2] });
            el.copyFormat(false);
            el.setSelection({ para: +f[3], offset: +f[4] }, { para: +f[5], offset: +f[6] });
            el.applyPainter();
            break;
          }
          case 'tg': el.toggleRunProp(T.TOGGLES[+f[1]]); break;
          case 'tu': el.toggleUnderline(f[1] === '-' ? undefined : f[1]); break;
          case 'su': el.setUnderline(T.UL[+f[1]]); break;
          case 'uc': el.setUnderlineColor(colorOf(f[1])); break;
          case 'fx': el.setTextEffects(clone(T.FX[+f[1]])); break;
          case 'fp': el.applyTextEffectPreset(+f[1] < 0 ? null : +f[1]); break;
          case 'ot': el.setOpenType(clone(T.OT[+f[1]])); break;
          case 'tgr': el.setTextGradient(clone(T.GRAD[+f[1]])); break;
          case 'ff': el.setFontFormat(clone(T.FF[+f[1]])); break;
          case 'va': el.toggleVertAlign(+f[1] ? 'subscript' : 'superscript'); break;
          case 'fs': el.setFontSize(T.SIZES[+f[1]]); break;
          case 'gf': el.growFont(+f[1]); break;
          case 'fam': el.setFontFamily(T.FAMILIES[+f[1]]); break;
          case 'co': el.setColor(colorOf(f[1]) ?? undefined); break;
          case 'hl': el.setHighlight(T.HL[+f[1]] === '-' ? undefined : T.HL[+f[1]]); break;
          case 'cf': el.clearFormatting(); break;
          case 'cc': el.changeCase(T.CASES[+f[1]]); break;
          case 'cs': el.setCharSpacing(f[1] === '-' ? undefined : +f[1]); break;
          case 'ss': { el.selectSimilarFormatting(); const s = el.selection; el.setSelection(s.anchor, s.focus); break; }
          case 'al': el.setAlignment(T.ALIGN[+f[1]]); break;
          case 'in': el.indent(+f[1]); break;
          case 'ls': el.setLineSpacing(+f[1]); break;
          case 'sa': el.setSpaceAfter(f[1] === '-' ? undefined : +f[1]); break;
          case 'sb': el.setSpaceBefore(f[1] === '-' ? undefined : +f[1]); break;
          case 'sh': el.setShading(f[1] === '-' ? undefined : f[1]); break;
          case 'si': el.setSpecialIndent(['none', 'firstLine', 'hanging'][+f[1]]); break;
          case 'ir': el.setIndentRight(f[1] === '-' ? undefined : +f[1]); break;
          case 'pi': el.setParagraphIndents(clone(T.INDENTS[+f[1]])); break;
          case 'ts': { const t = clone(T.TABS[+f[1]]); el.setTabStops(t.tabs, t.only); break; }
          case 'pf': el.setParagraphFlow(clone(T.FLOW[+f[1]])); break;
          case 'pl': el.setParagraphLayout(clone(T.LAYOUT[+f[1]])); break;
          case 'pb': el.setParagraphBorders(clone(T.BORDERS[+f[1]])); break;
          case 'bp': el.paragraphBorderPreset(T.PRESETS[+f[1]]); break;
          case 'tb': el.toggleBorder(); break;
          case 'hr': el.insertHorizontalLine(); break;
          case 'cp': el.clearParagraphFormatting(); break;
        }
      }, [code, T]);
    }
    out.set(key, 'op', code);
    const st = await page.evaluate(() => {
      const el = window.__el;
      const enc = (v) => (v instanceof Uint8Array ? { __u8: btoa(String.fromCharCode(...v)) } : v);
      const walk = (v) => (v && typeof v === 'object' && !(v instanceof Uint8Array) ? (Array.isArray(v) ? v.map(walk) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(enc(x))]))) : enc(v));
      const state = el.parseState(el.stringifyState());
      // declared normalisation: the horizontal-line shape has no `geom` in the editor
      for (const p of state.m.paragraphs) for (const r of p.runs) if (r.shape?.hr && r.shape.style && r.shape.style.geom === undefined) r.shape.style.geom = 'rect';
      return {
        paras: walk(state.m.paragraphs),
        sel: JSON.stringify(state.s),
        anc: JSON.stringify(state.anc ?? null),
        ev: window.__ev.join(' '),
        flags: `${el.isDirty() ? 1 : 0}${el.undoStack.length ? 1 : 0}${el.redoStack.length ? 1 : 0}:${el.editsSinceLoad}`,
        pend: JSON.stringify(el.pendingProps ?? null),
      };
    });
    if (DUMP && DUMP.startsWith(`${key}:`) && !DUMP.endsWith(':xml')) {
      const para = Number(DUMP.slice(key.length + 1));
      console.log(JSON.stringify(revive(st.paras)[para], (k, v) => (v instanceof Uint8Array ? `#${v.length}` : v)));
      process.exit(0);
    }
    out.set(key, 'paras', digest(revive(st.paras)));
    out.set(key, 'sel', st.sel);
    out.set(key, 'anc', st.anc);
    out.set(key, 'ev', st.ev || '-');
    out.set(key, 'flags', st.flags);
    out.set(key, 'pend', st.pend);
    if (j % 8 === 0 || j === STEPS) {
      const b64 = await page.evaluate(() => { try { const b = window.__el.saveDocx(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000)); return btoa(s); } catch { return null; } });
      let d = 'err';
      if (b64) {
        const pkg = DocxPackage.load(Uint8Array.from(Buffer.from(b64, 'base64')));
        d = Digest.of(Buffer.from(pkg.getPartText('word/document.xml') ?? '', 'utf8')).short;
        if (DUMP === `${key}:xml`) { console.log(pkg.getPartText('word/document.xml')); process.exit(0); }
      }
      out.set(key, 'docxml', d);
    }
  }
  await page.close();
  n++;
}
await browser.close();
server.close();
out.write(join(crate, 'tests', 'parity', 'd2b-f1-session.capture'));
console.log(`d2b-f1-session: ${n} documents x ${STEPS} steps`);
