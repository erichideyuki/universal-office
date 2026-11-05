// D2b-F4 (sections, page setup, headers/footers, page colour, text layout options) in the D2 editor
// harness (G17, U5, U16): the frozen `<universal-write>` element in headless Chromium (Playwright),
// driven through its public API and the TS-private entry points the D2a harness uses (`type`,
// `pressEnter`, `pressBackspace`, `stringifyState`, `parseState`). The page set-up is the one of
// `capture-d2a-session.mjs` (a script with top-level effects: it cannot be imported, so the server
// and browser launch are repeated here): the element initialises with the open default font so
// that the layout pass runs, `Math.random` = the testkit xorshift32, `Date` fixed (2026-01-02 03:04
// UTC), timezone UTC.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f4-session.mjs [--docs N] [--steps N]
//
// Writes tests/parity/d2b-f4-session.capture: per `<doc>#<j>` the D2a facets (op, paras, sel, anc,
// ev, flags, docxml) and F4's own:
//   sec    the effective sections (`fullDoc.sections`): geometry, type, columns, line numbers, vertical
//          alignment, page-number format, header/footer distances, the `sect` props the layout reads and
//          the header/footer references;
//   body   the body `sectPr` and `bodyRefs` of the undo state (`m.sectPr`, `m.bodyRefs`);
//   st     the settings the editor edits (even/odd, default tab, hyphenation, background, display flag);
//   hfp    the header/footer parts of the index: key, path and the text of each paragraph;
//   ctx    the part in edit (`hfEdit`);
//   parts  every 8 steps: the saved settings.xml, header/footer parts, their relationships, the content
//          types, styles.xml;
// and the layout answers the Rust replay gets as arguments (recorded BEFORE the op): `hc`
// (`hfContext()`: section and variant), `hp` (the page's section and variant), `hs` (the section of
// the page in header/footer edit); and AFTER it: `reenter` (the path of the part in edit), `cr`
// (the part a `enterHeaderFooter` created: kind, section, variant).
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { join, extname, normalize } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter, Rng, Stream } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const Digest = { of: (b) => ({ short: createHash('sha256').update(b).digest('hex').slice(0, 32) }) };
const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 40);
const STEPS = opt('--steps', 60);
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
const TEXTS = ['a', 'Olá ', ' ', 'ção', '😀', 'abc def'];
const ST = { n: 'nextPage', c: 'continuous', k: 'nextColumn', e: 'evenPage', o: 'oddPage' };
const AP = { s: 'section', d: 'document', f: 'forward', x: 'selection' };
const VA = { t: 'top', c: 'center', b: 'bottom', B: 'both' };
const MODES = { n: 'none', c: 'continuous', p: 'page', s: 'section' };
const PRESETS = { 1: 'one', 2: 'two', 3: 'three', l: 'left', r: 'right' };
const GS = ['horizontal', 'vertical', 'diagonalUp', 'diagonalDown', 'fromCorner', 'fromCenter'];
const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
const revive = (v) => (v && typeof v === 'object' ? (v.__u8 !== undefined ? Uint8Array.from(Buffer.from(v.__u8, 'base64')) : (Array.isArray(v) ? v.map(revive) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, revive(x)])))) : v);

function digest(paras) {
  const s = new Stream();
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  return s.finish().short;
}
const sha = (t) => Digest.of(Buffer.from(t, 'utf8')).short;

const out = new CaptureWriter([
  'D2b-F4 session in Chromium: the frozen <universal-write> driven by seeded scripts (page setup, section and page breaks, columns, line numbers, headers/footers, page colour, default tab, hyphenation)',
  'facets: op, paras, sel, anc, ev, flags, docxml, sec, body, st, hfp, ctx, parts, hc, hp, hs, reenter, cr',
]);
out.meta('steps', STEPS);
out.meta('browser', `chromium ${browser.version()}`);
out.meta('bundle', Digest.of(readFileSync(join(root, 'write/packages/editor/publish/index.js'))).short);
out.meta('math-override', 'none (no F4 state value goes through Math beyond round/floor/min/max)');

function pick() {
  const all = corpus();
  const rich = [];
  const plain = [];
  for (const f of all) {
    let t = '';
    try {
      const pkg = DocxPackage.load(readBytes(f.file));
      t = pkg.getPartText('word/document.xml') ?? '';
      // a table in a header/footer: the TS edits the part through the flattened cell paragraphs (the
      // editing list of the D2a-C1 family: for the gate), the session through its top-level ones
      if (pkg.partNames().some((nm) => /^word\/(header|footer)[^/]*\.xml$/.test(nm) && /<w:tbl[ >]/.test(pkg.getPartText(nm) ?? ''))) continue;
    } catch { continue; }
    ((t.match(/<w:sectPr\b/g) ?? []).length > 1 || /<w:headerReference\b|<w:footerReference\b/.test(t) ? rich : plain).push(f);
  }
  const take = (list, n, mod) => list.filter((_, i) => i % mod === 0).slice(0, n);
  return [...take(rich, Math.ceil(DOCS * 0.7), 1), ...take(plain, Math.floor(DOCS * 0.3), 17)].slice(0, DOCS);
}
const files = pick();
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
  let seed = 0xf4f4;
  for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
  const rng = new Rng(seed);
  const info = () => page.evaluate(() => ({ lens: window.__el.model.paragraphs.map((p) => p.runs.reduce((a, r) => a + r.text.length, 0)), hf: !!window.__el.hfEdit }));
  for (let j = 0; j <= STEPS; j++) {
    const key = `${rel}#${j}`;
    let code = 'init';
    const lay = {};
    if (j > 0) {
      const I = await info();
      const L = I.lens;
      const pos = () => { const para = rng.below(L.length); return { para, offset: rng.below(L[para] + 1) }; };
      const k = rng.below(100);
      const b = () => rng.below(2);
      const ap = () => ['s', 'd', 'f', 'x'][rng.below(4)];
      const st = () => ['n', 'c', 'k', 'e', 'o'][rng.below(5)];
      const kindC = () => (rng.below(2) ? 'h' : 'f');
      if (k < 8) code = `ty:${hex(TEXTS[rng.below(TEXTS.length)])}`;
      else if (k < 11) code = 'en';
      else if (k < 14) code = 'bs';
      else if (k < 21) { const a = pos(); const f = rng.below(2) === 0 ? a : pos(); code = `se:${a.para}:${a.offset}:${f.para}:${f.offset}`; }
      else if (k < 23) code = `tk:${b()}`;
      else if (k < 31) {
        const f = [];
        if (b()) f.push(`o=${b() ? 'L' : 'P'}`);
        if (b()) f.push(`s=${b() ? 'A' : 'L'}`);
        if (b()) f.push(`m=${['n', 'r', 'w'][rng.below(3)]}`);
        if (rng.below(4) === 0) f.push('cm=1000.1100.1200.1300');
        if (b()) f.push(`st=${st()}`);
        if (b()) f.push(`va=${['t', 'c', 'b', 'B'][rng.below(4)]}`);
        if (b()) f.push(`ln=${b()}.${1 + rng.below(5)}.${1 + rng.below(3)}.${[0, 300][rng.below(2)]}.${['p', 's', 'c'][rng.below(3)]}`);
        if (b()) f.push(`ap=${ap()}`);
        code = `ps:${f.join(',') || 'o=L'}`;
      }
      else if (k < 36) code = `sb:${st()}`;
      else if (k < 38) code = 'cb';
      else if (k < 41) code = 'pgb';
      else if (k < 42) code = 'bp';
      else if (k < 47) code = `col:${['1', '2', '3', 'l', 'r'][rng.below(5)]}`;
      else if (k < 50) code = `cs:${1 + rng.below(4)}:${b() ? 300 * rng.below(6) : '-'}:${b()}:${ap()}:${b()}:${rng.below(3) === 0 ? 1 : 0}`;
      else if (k < 53) code = `ss:${st()}:${ap()}`;
      else if (k < 55) code = `va:${['t', 'c', 'b', 'B'][rng.below(4)]}:${ap()}`;
      else if (k < 58) code = `ln:${['n', 'c', 'p', 's'][rng.below(4)]}`;
      else if (k < 60) code = `lo:${b()}.${1 + rng.below(5)}.${1 + rng.below(3)}.${[0, 300][rng.below(2)]}.${['p', 's', 'c'][rng.below(3)]}:${ap()}`;
      else if (k < 61) code = 'ls';
      else if (k < 62) code = `pb:${b() ? 'd' : 's'}`;
      else if (k < 65) code = `dfp:${b()}`;
      else if (k < 67) code = `doe:${b()}`;
      else if (k < 69) code = `hfd:${b() ? 100 * rng.below(9) : '-'}:${b() ? 100 * rng.below(9) : '-'}`;
      else if (k < 74) code = `ltp:${b()}:${['h', 'f', '-'][rng.below(3)]}`;
      else if (k < 76) code = `rh:${kindC()}`;
      else if (k < 77) code = 'rpn';
      else if (k < 79) code = `pnf:${b() ? ['decimal', 'lowerRoman', 'upperLetter'][rng.below(3)] : '-'}:${['-', 'none', `${1 + rng.below(9)}`][rng.below(3)]}:${['-', 'none', `${rng.below(5)}`][rng.below(3)]}`;
      else if (k < 80) code = `dt:${100 + rng.below(1000)}`;
      else if (k < 82) code = `hy:${b()}`;
      else if (k < 84) code = `ho:${['-', '0', '1'][rng.below(3)]}.${['-', '0', '1'][rng.below(3)]}.${['-', '0', '100', '425'][rng.below(4)]}.${['-', '0', '2'][rng.below(3)]}`;
      else if (k < 86) code = `pc:${['none', 'ffcc00', '336699', 't_accent1_-_-', 't_text2_0.4_-', 't_background1_-_0.25'][rng.below(6)]}`;
      else if (k < 88) {
        const gsx = rng.below(6);
        const v = 1 + rng.below(4);
        code = `pf:${['none', `g1_4472C4_${[0, 0.5, 0.8][rng.below(3)]}_${gsx}_${v}`, `g2_4472C4_FFC000_${gsx}_${v}`, `gp_${['mist', 'aurora', 'none'][rng.below(3)]}_${gsx}_${v}`, `pat_${['pct50', 'wave', 'nope'][rng.below(3)]}_FF0000_00FF00`, `pic_${b()}`][rng.below(6)]}`;
      }
      else if (k < 91) code = `ehf:${kindC()}`;
      else if (k < 93) code = 'xhf';
      else if (k < 97) code = 'un';
      else code = 're';
      // the layout answers the Rust replay takes as arguments (before the op)
      Object.assign(lay, await page.evaluate(() => {
        const el = window.__el;
        const r = {};
        try {
          const c = el.hfContext();
          r.hc = c ? `${c.sec}:${c.type}` : '-';
          const pg = el.caretPageIndex();
          r.hp = `${el.pageSections[pg] ?? 0}:${el.hfTypeOfPage(pg)}`;
          r.hs = el.hfEdit ? String(el.pageSections[el.hfActivePage] ?? 0) : '-';
        } catch { r.hc = '-'; r.hp = '-'; r.hs = '-'; }
        r.keys0 = [...el.hfParts.keys()].join(',');
        return r;
      }));
      await page.evaluate(([code, ST, AP, VA, MODES, PRESETS, GS]) => {
        const el = window.__el;
        window.__ev = [];
        const f = code.split(':');
        const num = (x) => (x === '-' ? undefined : +x);
        const kindOf = (x) => (x === 'h' ? 'header' : x === 'f' ? 'footer' : undefined);
        switch (f[0]) {
          case 'ty': el.type(String.fromCharCode(...f[1].split('.').map((h) => parseInt(h, 16)))); break;
          case 'en': el.pressEnter(); break;
          case 'bs': el.pressBackspace(); break;
          case 'se': el.setSelection({ para: +f[1], offset: +f[2] }, { para: +f[3], offset: +f[4] }); break;
          case 'tk': el.toggleTrackChanges(f[1] === '1'); break;
          case 'ps': {
            const o = {};
            for (const kv of f[1].split(',')) {
              const [k, v] = kv.split('=');
              if (k === 'o') o.orientation = v === 'L' ? 'landscape' : 'portrait';
              else if (k === 's') o.size = v === 'A' ? 'A4' : 'Letter';
              else if (k === 'm') o.margins = { n: 'normal', r: 'narrow', w: 'wide' }[v];
              else if (k === 'cm') { const [t, r, b, l] = v.split('.').map(Number); o.customMargins = { top: t, right: r, bottom: b, left: l }; }
              else if (k === 'st') o.sectionStart = ST[v];
              else if (k === 'va') o.verticalAlignment = VA[v];
              else if (k === 'ln') { const [on, st, by, d, rs] = v.split('.'); o.lineNumbering = { on: on === '1', startAt: +st, countBy: +by, distanceTwips: +d, restart: { p: 'page', s: 'section', c: 'continuous' }[rs] }; }
              else if (k === 'ap') o.applyTo = AP[v];
            }
            el.setPageSetup(o);
            break;
          }
          case 'sb': el.insertSectionBreak(ST[f[1]]); break;
          case 'cb': el.insertColumnBreak(); break;
          case 'pgb': el.insertPageBreak(); break;
          case 'bp': el.insertBlankPage(); break;
          case 'col': el.setColumns(PRESETS[f[1]]); break;
          case 'cs': el.setColumns({ count: +f[1], ...(f[2] !== '-' ? { spaceTwips: +f[2] } : {}), sep: f[3] === '1', applyTo: AP[f[4]], startNewColumn: f[5] === '1', ...(f[6] === '1' ? { widths: [{ widthTwips: 3000, spaceTwips: 300 }, { widthTwips: 2500, spaceTwips: 200 }, { widthTwips: 2000, spaceTwips: 100 }, { widthTwips: 900, spaceTwips: 0 }] } : {}) }); break;
          case 'ss': el.setSectionStart(ST[f[1]], AP[f[2]]); break;
          case 'va': el.setVerticalAlignment(VA[f[1]], AP[f[2]]); break;
          case 'ln': el.setLineNumbering(MODES[f[1]]); break;
          case 'lo': { const [on, st, by, d, rs] = f[1].split('.'); el.setLineNumberingOptions({ on: on === '1', startAt: +st, countBy: +by, distanceTwips: +d, restart: { p: 'page', s: 'section', c: 'continuous' }[rs] }, AP[f[2]]); break; }
          case 'ls': el.toggleLineNumberSuppression(); break;
          case 'pb': el.setPageBorders(null, f[1] === 'd' ? 'document' : 'section'); break;
          case 'dfp': el.setDifferentFirstPage(f[1] === '1'); break;
          case 'doe': el.setDifferentOddEven(f[1] === '1'); break;
          case 'hfd': el.setHeaderFooterDistance({ ...(f[1] !== '-' ? { header: +f[1] } : {}), ...(f[2] !== '-' ? { footer: +f[2] } : {}) }); break;
          case 'ltp': el.setLinkToPrevious(f[1] === '1', kindOf(f[2])); break;
          case 'rh': el.removeHeaderFooter(kindOf(f[1])); break;
          case 'rpn': el.removePageNumbers(); break;
          case 'pnf': {
            const o = {};
            if (f[1] !== '-') o.format = f[1];
            if (f[2] === 'none') o.chapter = null; else if (f[2] !== '-') o.chapter = { level: +f[2], separator: 'hyphen' };
            if (f[3] === 'none') o.start = null; else if (f[3] !== '-') o.start = +f[3];
            el.setPageNumberFormat(o);
            break;
          }
          case 'dt': el.setDefaultTabStop(+f[1]); break;
          case 'hy': el.setHyphenation(f[1] === '1' ? 'auto' : 'none'); break;
          case 'ho': { const [a, c, z, l] = f[1].split('.'); const o = {}; if (a !== '-') o.auto = a === '1'; if (c !== '-') o.caps = c === '1'; if (z !== '-') o.zoneTwips = +z; if (l !== '-') o.limit = +l; el.setHyphenationOptions(o); break; }
          case 'pc': {
            if (f[1] === 'none') el.setPageColor(null);
            else if (f[1].startsWith('t_')) { const [, tc, tint, shade] = f[1].split('_'); el.setPageColor({ themeColor: tc, ...(tint !== '-' ? { tint: +tint } : {}), ...(shade !== '-' ? { shade: +shade } : {}) }); }
            else el.setPageColor(f[1]);
            break;
          }
          case 'pf': {
            const g = f[1].split('_');
            if (f[1] === 'none') el.setPageFill(null);
            else if (g[0] === 'g1') el.setPageFill({ type: 'gradient', colors: 'one', color: g[1], degree: +g[2], style: GS[+g[3]], variant: +g[4] });
            else if (g[0] === 'g2') el.setPageFill({ type: 'gradient', colors: 'two', color: g[1], color2: g[2], style: GS[+g[3]], variant: +g[4] });
            else if (g[0] === 'gp') el.setPageFill({ type: 'gradient', colors: 'preset', preset: g[1], style: GS[+g[2]], variant: +g[3] });
            else if (g[0] === 'pat') el.setPageFill({ type: 'pattern', pattern: g[1], color: g[2], background: g[3] });
            else if (g[0] === 'pic') el.setPageFill({ type: g[1] === '1' ? 'texture' : 'picture', data: new Uint8Array([1, 2, 3]), ext: 'PNG', name: 'a.b.png' });
            break;
          }
          case 'ehf': el.enterHeaderFooter(kindOf(f[1]), 0); break;
          case 'xhf': el.exitHeaderFooter(); break;
          case 'un': el.undo(); break;
          case 're': el.redo(); break;
        }
      }, [code, ST, AP, VA, MODES, PRESETS, GS]);
    }
    out.set(key, 'op', code);
    const st = await page.evaluate((keys0) => {
      const el = window.__el;
      const enc = (v) => (v instanceof Uint8Array ? { __u8: btoa(String.fromCharCode(...v)) } : v);
      const walk = (v) => (v && typeof v === 'object' && !(v instanceof Uint8Array) ? (Array.isArray(v) ? v.map(walk) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(enc(x))]))) : enc(v));
      const state = el.parseState(el.stringifyState());
      const hx = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
      const f = (v) => (v === undefined ? '-' : v === null ? 'N' : typeof v === 'object' ? JSON.stringify(v) : String(v));
      const lnS = (l) => (l ? [l.countBy, l.start, l.distance, l.restart].map(f).join('/') : '-');
      const colsS = (c) => (c ? `${f(c.count)}/${f(c.spaceTwips)}/${f(c.sep)}/${(c.widths ?? []).map((w) => `${f(w.widthTwips)}.${f(w.spaceTwips)}`).join(';')}` : '-');
      const spS = (sp) => [sp.pageWidthTwips, sp.pageHeightTwips, sp.marginLeftTwips, sp.marginRightTwips, sp.marginTopTwips, sp.marginBottomTwips, sp.type, sp.titlePg, sp.headerDistanceTwips, sp.footerDistanceTwips, sp.pgNumStart, sp.pgNumFmt, sp.pgNumChapStyle, sp.pgNumChapSep, sp.vAlign].map(f).join('|') + `|${colsS(sp.cols)}|${lnS(sp.lnNumType)}|${sp.pgBorders === undefined ? 'u' : sp.pgBorders === null ? 'n' : 'b'}`;
      const refsS = (rs) => rs.map((r) => `${r.type}.${r.rid}.${r.path}`).join(';');
      const sections = (el.fullDoc?.sections ?? []).map((s) => {
        const e = s.sect ?? {};
        return `${spS(s.sectPr)}#${[e.type, e.vAlign, e.equalWidth].map(f).join('|')}|${lnS(e.lnNumType)}|${(e.colDefs ?? []).map((d) => `${f(d.w)}.${f(d.space)}`).join(';')}#${refsS(s.headerRefs)}#${refsS(s.footerRefs)}`;
      });
      const br = state.m.bodyRefs;
      const bodyS = `${spS(state.m.sectPr)}#${br && (br.headerRefs.length || br.footerRefs.length) ? `${br.headerRefs.map((r) => `${r.type}.${r.rid}`).join(';')}/${br.footerRefs.map((r) => `${r.type}.${r.rid}`).join(';')}` : '-'}`;
      const bg = el.fullDoc?.background;
      const fl = bg?.fill;
      const bgS = bg ? [bg.color, bg.themeColor, bg.themeTint, bg.themeShade].map(f).join('|') + (fl ? `|${[fl.type, fl.color, fl.color2, fl.angle, fl.focus, fl.focusPosition, fl.gradientCenter, fl.method, fl.title].map(f).join('/')}/${(fl.colors ?? []).map((c) => `${c.pos}.${c.color}`).join(';')}/${fl.image ? `${fl.image.data.length}.${fl.image.ext}` : '-'}` : '') : '-';
      const hy = el.hyphenationSettings();
      const stS = [el.evenOddHeaders() ? 1 : 0, el.defaultTabStop(), `${hy.auto ? 1 : 0}${hy.caps ? 1 : 0}.${f(hy.zone)}.${f(hy.limit)}`, bgS, el.fullDoc?.formatting?.settings.displayBackgroundShape ? 1 : 0].join('|');
      // the top-level paragraphs of a part: its paragraphs but those of a table's cells
      const topParas = (p, paras) => {
        const inTable = new Set();
        const walk = (bs) => { for (const b of bs ?? []) if (b.type === 'table') for (const r of b.rows) for (const c of r.cells) for (const q of c.paragraphs) { inTable.add(q); if (c.blocks) walk(c.blocks); } };
        walk(p.blocks);
        return (paras ?? []).filter((q) => !inTable.has(q));
      };
      const hfp = [...el.hfParts.entries()].map(([k, p]) => `${k}|${p.path}|${topParas(p, (el.hfEdit && k === `${el.hfEdit.kind}:${el.hfEdit.rid}`) ? el.model.paragraphs : p.paras).map((q) => q.runs.map((r) => (r.field ? '#' : r.text)).join('')).map(hx).join(';')}`);
      const ctx = el.hfEdit ? `${el.hfEdit.kind}:${el.hfEdit.rid}:${el.hfEdit.path}:${el.hfEdit.type}` : '-';
      // a part created by `enterHeaderFooter`: kind, section (index of the model's references) and variant
      let cr = '-';
      const now = [...el.hfParts.keys()];
      const old = new Set(keys0 ? keys0.split(',') : []);
      const fresh = now.filter((k) => !old.has(k));
      if (keys0 !== null && fresh.length === 1) {
        const [kind, rid] = fresh[0].split(':');
        const m = el.bodyModel();
        const count = m.paragraphs.filter((p) => p.props.sectionBreak).length + 1;
        for (let sIdx = 0; sIdx < count; sIdx++) {
          const holder = el.refsOfSection(m, sIdx, kind).find((r) => r.rid === rid);
          if (holder) { cr = `${kind}:${sIdx}:${holder.type}`; break; }
        }
      }
      return {
        paras: walk(state.m.paragraphs),
        sel: JSON.stringify(state.s),
        anc: JSON.stringify(state.anc ?? null),
        ev: window.__ev.join(' '),
        flags: `${el.isDirty() ? 1 : 0}${el.undoStack.length ? 1 : 0}${el.redoStack.length ? 1 : 0}:${el.editsSinceLoad}`,
        sec: sections.join('\n'),
        body: bodyS,
        st: stS,
        hfp: hfp.join('\n'),
        ctx,
        reenter: el.hfEdit ? el.hfEdit.path : '-',
        cr,
      };
    }, j > 0 ? (lay.keys0 ?? '') : null);
    out.set(key, 'paras', digest(revive(st.paras)));
    if (process.env.UO_F4_TEXT) out.set(key, 'paras.text', revive(st.paras).map((p) => { const s = new Stream(); new Canon(s).value(p, 'paragraph'); return s.finish().short.slice(0, 6); }).join(','));
    out.set(key, 'sel', st.sel);
    out.set(key, 'anc', st.anc);
    out.set(key, 'ev', st.ev || '-');
    out.set(key, 'flags', st.flags);
    out.set(key, 'sec', sha(st.sec));
    out.set(key, 'body', sha(st.body));
    out.set(key, 'st', st.st);
    out.set(key, 'hfp', sha(st.hfp));
    out.set(key, 'ctx', st.ctx);
    out.set(key, 'reenter', st.reenter);
    out.set(key, 'cr', st.cr);
    if (j > 0) { out.set(key, 'hc', lay.hc ?? '-'); out.set(key, 'hp', lay.hp ?? '-'); out.set(key, 'hs', lay.hs ?? '-'); }
    const nl = (t) => t.replace(/[\t\r\n]/g, (c) => (c === '\n' ? '\u23CE' : ' '));
    if (process.env.UO_F4_TEXT) { out.set(key, 'sec.text', nl(st.sec)); out.set(key, 'body.text', nl(st.body)); out.set(key, 'hfp.text', nl(st.hfp)); }
    if (j % 8 === 0 || j === STEPS) {
      const b64 = await page.evaluate(() => { try { const b = window.__el.saveDocx(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000)); return btoa(s); } catch { return null; } });
      let d = 'err';
      let parts = 'err';
      if (b64) {
        const pkg = DocxPackage.load(Uint8Array.from(Buffer.from(b64, 'base64')));
        d = Digest.of(Buffer.from(pkg.getPartText('word/document.xml') ?? '', 'utf8')).short;
        const names = pkg.partNames().filter((nm) => /^word\/(header|footer)[^/]*\.xml$|^word\/settings\.xml$|^word\/styles\.xml$|^word\/_rels\/(document|header|footer)[^/]*\.xml\.rels$|^\[Content_Types\]\.xml$/.test(nm)).sort();
        parts = sha(names.map((nm) => `${nm}\n${pkg.getPartText(nm) ?? ''}`).join('\n#\n'));
        if (process.env.UO_F4_TEXT) out.set(key, 'parts.text', names.map((nm) => `${nm}\n${pkg.getPartText(nm) ?? ''}`).join('\n#\n').replace(/[\t\r\n]/g, (c) => (c === '\n' ? '\u23CE' : ' ')));
      }
      out.set(key, 'docxml', d);
      out.set(key, 'parts', parts);
    }
  }
  await page.close();
  n++;
}
await browser.close();
server.close();
out.write(join(crate, 'tests', 'parity', 'd2b-f4-session.capture'));
console.log(`d2b-f4-session: ${n} documents x ${STEPS} steps`);
