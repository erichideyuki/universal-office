// The D2b-F2 editor harness (lists and styles; record uo-docx.md "D2a — session (architecture)",
// "D2b frontier briefs", and uo-docx-d2b/F2.md): the frozen `<universal-write>` element in headless
// Chromium (Playwright), as `capture-d2a-session.mjs` (same server, same init script, same facets),
// driven by seeded scripts of the F2 commands: toggleList, applyListLibrary, removeList,
// applyMultilevelLibrary, setListLevel, changeListLevel (Tab), restartNumbering, continueNumbering,
// setNumberingValue, defineListLevel, defineMultilevelList, createListStyle, applyListStyle,
// applyStyle, createStyle, modifyStyle, renameStyle, deleteStyle, setStyleInGallery,
// updateStyleFromSelection, setDefaultFont, the Style Inspector clears, plus typing, Enter (the
// list and next-style hooks), Backspace, selections, undo, redo and tracking.
//
// The D2a harness is a script (it runs on import), so its set-up is repeated here; the D2a file is
// untouched. The page locale is pt-BR, so the element's UI language is `pt` (the replay sets it).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f2-session.mjs [--docs N] [--steps N]
//
// Writes tests/parity/d2b-f2-session.capture: per `<doc>#<j>` the facets
//   op, paras, sel, anc, ev, flags  as the D2a session capture;
//   sv, nv                          short SHA-256 of the style sheet / numbering text of now
//                                   (`docStyles.xml`, `numbering.xml`; '-' = none);
//   docxml, numxml, stylexml        every 8 steps and at the end: the short SHA-256 of
//                                   `word/document.xml`, `word/numbering.xml`, `word/styles.xml`
//                                   of `saveDocx()` (the store saves, byte for byte).
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { join, extname, normalize } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter, Rng, Stream } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const sha = (b) => createHash('sha256').update(b).digest('hex').slice(0, 32);
const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 100);
const STEPS = opt('--steps', 60);
const STRIDE = opt('--stride', 23);
// `--only <substring>` keeps the documents whose path has it; `--dump <doc>#<step>:nv|sv` prints that text and stops
const ONLY = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : undefined;
const DUMP = argv.includes('--dump') ? argv[argv.indexOf('--dump') + 1] : undefined;
const { DocxPackage } = await ts('package.js');

const pwFrom = [join(root, 'slides', 'package.json'), join(root, 'sheet', 'package.json')].find((p) => existsSync(p));
const { chromium } = createRequire(pwFrom)('playwright');

const MIME = { '.js': 'text/javascript', '.mjs': 'text/javascript', '.html': 'text/html', '.json': 'application/json', '.wasm': 'application/wasm', '.css': 'text/css', '.ttf': 'font/ttf', '.otf': 'font/otf', '.woff2': 'font/woff2' };
const server = createServer((req, res) => {
  const url = decodeURIComponent((req.url ?? '/').split('?')[0]);
  if (url === '/__d2b.html') {
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
  let s = 1;
  Math.random = () => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s / 4294967296; };
  const T = Date.UTC(2026, 0, 2, 3, 4, 0);
  const D = Date;
  // eslint-disable-next-line no-global-assign
  Date = class extends D { constructor(...a) { if (a.length) super(...a); else super(T); } static now() { return T; } };
});

const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-input', 'uw-track-changes', 'uw-cellmode', 'uw-notemode', 'uw-hfmode', 'uw-tbmode', 'uw-protected', 'uw-blocked', 'uw-styles-change'];
const TEXTS = ['a', 'Olá ', ' ', 'ção', 'abc def'];
const NAMES = ['Meu Estilo', 'Destaque X', 'Citação', 'Nota 1'];
const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
const revive = (v) => (v && typeof v === 'object' ? (v.__u8 !== undefined ? Uint8Array.from(Buffer.from(v.__u8, 'base64')) : (Array.isArray(v) ? v.map(revive) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, revive(x)])))) : v);

function digest(paras) {
  const s = new Stream();
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  return s.finish().short;
}

const out = new CaptureWriter([
  'D2b-F2 session in Chromium: the frozen <universal-write> driven by seeded scripts of the lists and styles commands (and typing, Enter, Backspace, selection, undo, redo, tracking)',
  'facets: op, paras, sel, anc, ev, flags, sv, nv, docxml, numxml, stylexml',
]);
out.meta('steps', STEPS);
out.meta('browser', `chromium ${browser.version()}`);
out.meta('bundle', sha(readFileSync(join(root, 'write/packages/editor/publish/index.js'))));
out.meta('math-override', 'none (no F2 state value goes through math)');
out.meta('ui-lang', 'pt');

const files = corpus().filter((_, i) => i % STRIDE === 7).slice(0, DOCS).filter((f) => !ONLY || f.rel.includes(ONLY));
let n = 0;
for (const { rel, file } of files) {
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${port}/__d2b.html`);
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
  let seed = 0xf2f2;
  for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
  const rng = new Rng(seed);
  const info = () => page.evaluate(() => {
    const el = window.__el;
    return {
      lens: el.model.paragraphs.map((p) => p.runs.reduce((a, r) => a + r.text.length, 0)),
      styles: el.listStyles('all').map((s) => s.id),
      mine: el.listStyles('document').filter((s) => !s.builtIn).map((s) => s.id),
      lstyles: el.listStylesInDocument().map((s) => s.id),
    };
  });
  for (let j = 0; j <= STEPS; j++) {
    const key = `${rel}#${j}`;
    let code = 'init';
    if (j > 0) {
      const I = await info();
      const L = I.lens;
      const pos = () => { const para = rng.below(L.length); return { para, offset: rng.below(L[para] + 1) }; };
      const pick = (a) => a[rng.below(a.length)];
      const k = rng.below(100);
      if (k < 8) code = `ty:${hex(TEXTS[rng.below(TEXTS.length)])}`;
      else if (k < 14) code = 'en';
      else if (k < 20) code = 'bs';
      else if (k < 30) { const a = pos(); const f = rng.below(2) === 0 ? a : pos(); code = `se:${a.para}:${a.offset}:${f.para}:${f.offset}`; }
      else if (k < 32) code = `tk:${rng.below(2)}`;
      else if (k < 38) code = 'un';
      else if (k < 41) code = 're';
      else if (k < 46) code = `tl:${rng.below(2) ? 'b' : 'd'}`;
      else if (k < 52) code = `al:${rng.below(2) ? 'b' : 'n'}:${rng.below(8) === 0 ? '-' : rng.below(6)}`;
      else if (k < 55) code = 'rl';
      else if (k < 58) code = `ml:${rng.below(8) === 0 ? '-' : rng.below(7)}`;
      else if (k < 61) code = `sl:${rng.below(9)}`;
      else if (k < 65) code = `ci:${rng.below(2) ? 1 : -1}`;
      else if (k < 67) code = rng.below(2) ? 'rs' : `rs:${rng.below(5)}`;
      else if (k < 69) code = 'cn';
      else if (k < 71) code = `nv:${rng.below(2) ? 'n' : 'c'}:${rng.below(3) === 0 ? '-' : rng.below(6)}`;
      else if (k < 73) code = rng.below(2) ? `dl:b:${pick(['2022', '2013', 'f0b7', '2d'])}` : `dl:n:${pick(['upperRoman', 'lowerLetter', 'decimal', 'ordinal'])}:${pick(['%1)', '%1.', '(%1)'])}`;
      else if (k < 85) code = `as:${rng.below(6) === 0 ? '-' : pick(I.styles)}`;
      else if (k < 88) code = `cs:${rng.below(NAMES.length)}:${pick(['linked', 'paragraph', 'character'])}`;
      else if (k < 90) code = `ms:${pick(I.mine.length ? I.mine : I.styles)}:${rng.below(4)}`;
      else if (k < 91) code = `rn:${pick(I.mine.length ? I.mine : I.styles)}:${rng.below(NAMES.length)}`;
      else if (k < 92) code = `dd:${pick(I.mine.length ? I.mine : I.styles)}`;
      else if (k < 93) code = `sg:${pick(I.styles)}:${rng.below(2)}`;
      else if (k < 94) code = 'us';
      else if (k < 95) code = pick(['cp', 'cc', 'pf', 'cf']);
      else if (k < 96) code = `sf:${rng.below(4)}`;
      else if (k < 97) code = `ls:${rng.below(NAMES.length)}`;
      else if (k < 98) code = I.lstyles.length ? `la:${pick(I.lstyles)}` : 'rl';
      else if (k < 99) code = `dm:${rng.below(2)}`;
      else code = 'en';
      await page.evaluate(([code, NAMES]) => {
        const el = window.__el;
        window.__ev = [];
        const f = code.split(':');
        const lvl = (i, o) => ({ ilvl: i, numFmt: 'decimal', lvlText: `%${i + 1}.`, indLeft: 720 * (i + 1), hanging: 360, ...o });
        try {
          switch (f[0]) {
            case 'ty': el.type(String.fromCharCode(...f[1].split('.').map((h) => parseInt(h, 16)))); break;
            case 'en': el.pressEnter(); break;
            case 'bs': el.pressBackspace(); break;
            case 'de': el.pressDelete(); break;
            case 'se': el.setSelection({ para: +f[1], offset: +f[2] }, { para: +f[3], offset: +f[4] }); break;
            case 'tk': el.toggleTrackChanges(f[1] === '1'); break;
            case 'un': el.undo(); break;
            case 're': el.redo(); break;
            case 'tl': el.toggleList(f[1] === 'b' ? 'bullet' : 'decimal'); break;
            case 'al': el.applyListLibrary(f[1] === 'b' ? 'bullet' : 'number', f[2] === '-' ? null : +f[2]); break;
            case 'rl': el.removeList(); break;
            case 'ml': el.applyMultilevelLibrary(f[1] === '-' ? null : +f[1]); break;
            case 'sl': el.setListLevel(+f[1]); break;
            case 'ci': el.changeListLevel(+f[1]); break;
            case 'rs': el.restartNumbering(f[1] === undefined ? undefined : +f[1]); break;
            case 'cn': el.continueNumbering(); break;
            case 'nv': el.setNumberingValue({ mode: f[1] === 'n' ? 'new' : 'continue', ...(f[2] === '-' ? {} : { value: +f[2] }) }); break;
            case 'dl': el.defineListLevel(f[1] === 'b' ? { bullet: String.fromCharCode(parseInt(f[2], 16)), font: 'Arial' } : { numFmt: f[2], lvlText: f[3], bold: true }); break;
            case 'as': el.applyStyle(f[1] === '-' ? undefined : f[1]); break;
            case 'cs': el.createStyle({ name: NAMES[+f[1]], type: f[2] }); break;
            case 'ms': el.modifyStyle(f[1], [{ format: { bold: true } }, { format: { szHalfPt: 28, alignment: 'center' } }, { quick: false, autoUpdate: true }, { next: null, format: { italic: true, color: 'FF0000' } }][+f[2]]); break;
            case 'rn': el.renameStyle(f[1], NAMES[+f[2]]); break;
            case 'dd': el.deleteStyle(f[1]); break;
            case 'sg': el.setStyleInGallery(f[1], f[2] === '1'); break;
            case 'us': el.updateStyleFromSelection(); break;
            case 'cp': el.clearParagraphStyle(); break;
            case 'cc': el.clearCharacterStyle(); break;
            case 'pf': el.clearParagraphFormatting(); break;
            case 'cf': el.clearCharacterFormatting(); break;
            case 'sf': el.setDefaultFont([{ sizePt: 12 }, { fontFamily: 'Arial', bold: true }, { italic: true, color: '00FF00' }, { spacingTwips: 0 }][+f[1]]); break;
            case 'ls': el.createListStyle(NAMES[+f[1]], f[1] === '0' ? [] : [lvl(0, { numFmt: 'upperRoman' }), lvl(1, { numFmt: 'lowerLetter', lvlText: '%2)' })]); break;
            case 'la': el.applyListStyle(f[1]); break;
            case 'dm': el.defineMultilevelList([lvl(0, { numFmt: 'upperLetter' }), lvl(1, { lvlText: '%1.%2.' })], f[1] === '1' ? { applyTo: 'forward' } : {}); break;
          }
        } catch (e) { window.__ev.push(`THROW:${String(e).slice(0, 60)}`); }
      }, [code, NAMES]);
    }
    out.set(key, 'op', code);
    const st = await page.evaluate(() => {
      const el = window.__el;
      const enc = (v) => (v instanceof Uint8Array ? { __u8: btoa(String.fromCharCode(...v)) } : v);
      const walk = (v) => (v && typeof v === 'object' && !(v instanceof Uint8Array) ? (Array.isArray(v) ? v.map(walk) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(enc(x))]))) : enc(v));
      const state = el.parseState(el.stringifyState());
      return {
        paras: walk(state.m.paragraphs),
        sel: JSON.stringify(state.s),
        anc: JSON.stringify(state.anc ?? null),
        ev: window.__ev.join(' '),
        flags: `${el.isDirty() ? 1 : 0}${el.undoStack.length ? 1 : 0}${el.redoStack.length ? 1 : 0}:${el.editsSinceLoad}`,
        sv: el.docStyles.xml,
        nv: el.numbering.xml,
      };
    });
    if (DUMP && DUMP.startsWith(`${key}:`)) { console.log(DUMP.endsWith(':sv') ? st.sv : st.nv); process.exit(0); }
    out.set(key, 'paras', digest(revive(st.paras)));
    out.set(key, 'sel', st.sel);
    out.set(key, 'anc', st.anc);
    out.set(key, 'ev', st.ev || '-');
    out.set(key, 'flags', st.flags);
    out.set(key, 'sv', st.sv === null ? '-' : sha(Buffer.from(st.sv, 'utf8')));
    out.set(key, 'nv', st.nv === null ? '-' : sha(Buffer.from(st.nv, 'utf8')));
    if (j % 8 === 0 || j === STEPS) {
      const b64 = await page.evaluate(() => { try { const b = window.__el.saveDocx(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000)); return btoa(s); } catch { return null; } });
      const d = { docxml: 'err', numxml: 'err', stylexml: 'err' };
      if (b64) {
        const pkg = DocxPackage.load(Uint8Array.from(Buffer.from(b64, 'base64')));
        const h = (p) => { const t = pkg.getPartText(p); return t === undefined ? '-' : sha(Buffer.from(t, 'utf8')); };
        d.docxml = h('word/document.xml');
        d.numxml = h('word/numbering.xml');
        d.stylexml = h('word/styles.xml');
      }
      for (const [f, v] of Object.entries(d)) out.set(key, f, v);
    }
  }
  await page.close();
  n++;
}
await browser.close();
server.close();
out.write(join(crate, 'tests', 'parity', 'd2b-f2-session.capture'));
console.log(`d2b-f2-session: ${n} documents x ${STEPS} steps`);
