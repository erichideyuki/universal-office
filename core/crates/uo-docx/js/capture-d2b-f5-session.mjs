// D2b-F5 (footnotes and endnotes) in the D2 editor harness (G17, U5, U16): the frozen
// `<universal-write>` element in headless Chromium (Playwright), driven through its public API and
// the TS-private entry points the D2a harness uses (`type`, `pressEnter`, `pressBackspace`,
// `pressDelete`, `stringifyState`, `parseState`, plus `noteRefs`, `exitNote`, `enterCell`,
// `exitCell`, `syncNoteEdit`). The page set-up is the one of `capture-d2a-session.mjs` (a script
// with top-level effects: it cannot be imported, so the server and browser launch are repeated
// here): the element initialises with the open default font so that the layout pass runs,
// `Math.random` = the testkit xorshift32, `Date` fixed (2026-01-02 03:04 UTC), timezone UTC.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f5-session.mjs [--docs N] [--steps N]
//
// Writes tests/parity/d2b-f5-session.capture: per `<doc>#<j>` the D2a facets (op, paras, sel, anc,
// ev, flags, docxml) and F5's own:
//   csel   the selection inside a cell or a note (`ce.s` / `nt.s` of the state; `sel` is the body's);
//   ctx    the edit context (body, the note being edited, the cell);
//   no     `getNoteOptions` of both kinds and `noteCount` of both kinds;
//   np     `JSON.stringify(noteSettings)`;
//   ref    `noteRefs()` of the body (kind, id, paragraph, cell, offset);
//   nts    every note's content (after `syncNoteEdit`), by id: style, runs with their marks;
//   sect   the `footnotePr`/`endnotePr`/`footnoteColumns` of every section's `sectPr`;
//   parts  (every 8 steps, with the save) `footnotes.xml`, `endnotes.xml`, `settings.xml`,
//          `styles.xml`, `[Content_Types].xml` and the document rels.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, existsSync, readdirSync, writeFileSync } from 'node:fs';
import { join, extname, normalize } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter, Rng, Stream } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const Digest = { of: (b) => ({ short: createHash('sha256').update(b).digest('hex').slice(0, 32) }) };
const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 60);
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
const TEXTS = ['a', 'Olá ', ' ', 'ção', '😀', '𝒜!', 'abc def'];
const MARKS = [undefined, '*', '  ', ' † '];
// the option sets of `setNoteOptions` (defaults, reset, restart rules, rounding, the place)
const OPTS = [
  '{"numberFormat":"lowerLetter"}', '{"startAt":5}', '{"restart":"eachSect"}', '{"restart":"eachPage"}',
  '{"position":"beneathText"}', '{"position":"sectEnd"}', '{"columns":2}', '{"columns":0}',
  '{"numberFormat":"decimal","startAt":1,"restart":"continuous","position":"pageBottom"}', '{"startAt":0.4}',
  '{"startAt":-3}', '{"numberFormat":"upperRoman","restart":"eachSect","startAt":7}', '{"position":"docEnd"}', '{"numberFormat":"chicago","startAt":2.5}',
];
const MODES = ['toEndnotes', 'toFootnotes', 'swap'];
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
  'D2b-F5 session in Chromium: the frozen <universal-write> driven by seeded scripts (footnotes and endnotes: insert, options, convert, navigation, typing in a note, undo/redo)',
  'facets: op, paras, sel, anc, ev, flags, docxml, ctx, no, np, ref, nts, sect, parts',
]);
out.meta('steps', STEPS);
out.meta('browser', `chromium ${browser.version()}`);
out.meta('bundle', Digest.of(readFileSync(join(root, 'write/packages/editor/publish/index.js'))).short);
out.meta('math-override', 'none (no F5 state value goes through math)');

/** Documents with notes or several sections first, then the rest. */
function pick() {
  const all = corpus();
  const rich = [];
  const plain = [];
  for (const f of all) {
    let t = '';
    try { t = DocxPackage.load(readBytes(f.file)).getPartText('word/document.xml') ?? ''; } catch { continue; }
    (/<w:(footnoteReference|endnoteReference)\b/.test(t) || (t.match(/<w:sectPr\b/g) ?? []).length > 1 ? rich : plain).push(f);
  }
  const take = (list, n, mod) => list.filter((_, i) => i % mod === 0).slice(0, n);
  return [...take(rich, Math.ceil(DOCS * 0.7), 1), ...take(plain, Math.floor(DOCS * 0.3), 17)].slice(0, DOCS);
}
const only = process.env.UO_F5_ONLY;
const files = only ? corpus().filter((f) => f.rel.includes(only)) : pick();
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
  let seed = 0xf5f5;
  for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
  const rng = new Rng(seed);
  const info = () => page.evaluate(() => ({
    lens: window.__el.model.paragraphs.map((p) => p.runs.reduce((a, r) => a + r.text.length, 0)),
    refs: window.__el.noteRefs().map((r) => [r.kind, r.id]),
    tables: window.__el.tableAnchors.length,
    noteEdit: !!window.__el.noteEdit,
  }));
  for (let j = 0; j <= STEPS; j++) {
    const key = `${rel}#${j}`;
    let code = 'init';
    if (j > 0) {
      const I = await info();
      const L = I.lens;
      const pos = () => { const para = rng.below(L.length); return { para, offset: rng.below(L[para] + 1) }; };
      const k = rng.below(100);
      const ref = () => (I.refs.length && rng.below(8) ? rng.below(I.refs.length) : -1);
      if (k < 12) code = `ty:${hex(TEXTS[rng.below(TEXTS.length)])}`;
      else if (k < 15) code = 'en';
      else if (k < 20) code = 'bs';
      else if (k < 22) code = 'de';
      else if (k < 30) { const a = pos(); const f = rng.below(2) === 0 ? a : pos(); code = `se:${a.para}:${a.offset}:${f.para}:${f.offset}`; }
      else if (k < 33) code = `tk:${rng.below(2)}`;
      else if (k < 43) code = `nf:${rng.below(2)}:${rng.below(MARKS.length)}`;
      else if (k < 56) code = `no:${rng.below(2)}:${rng.below(OPTS.length)}:${rng.below(3) === 0 ? 1 : 0}`;
      else if (k < 62) code = `cv:${rng.below(3)}:${ref()}`;
      else if (k < 67) code = `on:${ref()}`;
      else if (k < 71) code = `gr:${ref()}`;
      else if (k < 77) code = `nn:${rng.below(2)}:${rng.below(2)}`;
      else if (k < 80) code = `sh:${rng.below(3)}`;
      else if (k < 84) code = 'xn';
      else if (k < 86 && I.tables && !I.noteEdit) code = `ec:${rng.below(I.tables)}`;
      else if (k < 88) code = 'xc';
      else if (k < 96) code = 'un';
      else code = 're';
      await page.evaluate(([code, OPTS, MARKS, MODES]) => {
        const el = window.__el;
        window.__ev = [];
        const f = code.split(':');
        const kind = (i) => (+i === 0 ? 'footnote' : 'endnote');
        const refAt = (i) => (+i >= 0 ? el.noteRefs()[+i] : undefined);
        switch (f[0]) {
          case 'ty': el.type(String.fromCharCode(...f[1].split('.').map((h) => parseInt(h, 16)))); break;
          case 'en': el.pressEnter(); break;
          case 'bs': el.pressBackspace(); break;
          case 'de': el.pressDelete(); break;
          case 'se': el.setSelection({ para: +f[1], offset: +f[2] }, { para: +f[3], offset: +f[4] }); break;
          case 'tk': el.toggleTrackChanges(f[1] === '1'); break;
          case 'nf': { const mark = MARKS[+f[2]]; const o = mark === undefined ? {} : { mark }; if (+f[1] === 0) el.insertFootnote(o); else el.insertEndnote(o); break; }
          case 'no': el.setNoteOptions(kind(f[1]), JSON.parse(OPTS[+f[2]]), f[3] === '1' ? 'section' : 'document'); break;
          case 'cv': { const r = refAt(f[2]); el.convertNotes(MODES[+f[1]], r ? { kind: r.kind, id: r.id } : undefined); break; }
          case 'on': { const r = refAt(f[1]); el.openNote(r ? r.kind : 'footnote', r ? r.id : 99999); break; }
          case 'gr': { const r = refAt(f[1]); el.goToNoteRef(r ? r.kind : 'footnote', r ? r.id : 99999); break; }
          case 'nn': el.nextNote(kind(f[1]), +f[2] === 1 ? 1 : -1); break;
          case 'sh': el.showNotes(+f[1] === 2 ? undefined : kind(f[1])); break;
          case 'xn': el.exitNote(); break;
          case 'ec': el.enterCell(+f[1], [{ r: 0, c: 0 }]); break;
          case 'xc': el.exitCell(); break;
          case 'un': el.undo(); break;
          case 're': el.redo(); break;
        }
      }, [code, OPTS, MARKS, MODES]);
    }
    out.set(key, 'op', code);
    const st = await page.evaluate(() => {
      const el = window.__el;
      el.syncNoteEdit();
      const enc = (v) => (v instanceof Uint8Array ? { __u8: btoa(String.fromCharCode(...v)) } : v);
      const walk = (v) => (v && typeof v === 'object' && !(v instanceof Uint8Array) ? (Array.isArray(v) ? v.map(walk) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(enc(x))]))) : enc(v));
      const state = el.parseState(el.stringifyState());
      const hx = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
      const fd = el.fullDoc;
      const runSig = (r) => `${hx(r.text)}|${r.props.charStyle ?? ''}|${r.props.vertAlign ?? ''}|${r.props.ins ? r.props.ins.author : ''}|${r.props.del ? r.props.del.author : ''}|${r.field ? hx(r.field) : ''}|${r.noteRef ? `${r.noteRef.kind}:${r.noteRef.id}:${r.noteRef.custom ? 1 : 0}` : ''}`;
      const paraSig = (p) => `${p.props.styleId ?? ''}:${p.runs.map(runSig).join(',')}`;
      const noteSig = (map) => [...(map ?? new Map()).entries()].sort((a, b) => a[0] - b[0]).map(([id, ps]) => `${id}=${ps.map(paraSig).join(';')}`).join('\n');
      const sectOf = (sp) => `${JSON.stringify(sp.footnotePr ?? null)}|${JSON.stringify(sp.endnotePr ?? null)}|${sp.footnoteColumns ?? ''}`;
      const body = el.bodyState().model;
      const sects = [];
      for (const p of body.paragraphs) if (p.props.sectionBreak) sects.push(sectOf(p.props.sectionBreak.sectPr));
      sects.push(sectOf(body.sectPr));
      const opts = (k) => { const o = el.getNoteOptions(k); return `${o.position},${o.numberFormat},${o.startAt},${o.restart},${o.columns ?? '-'}`; };
      const ne = el.editingNote();
      return {
        paras: walk(state.m.paragraphs),
        sel: JSON.stringify(state.s),
        csel: JSON.stringify((state.ce ?? state.nt)?.s ?? null),
        anc: JSON.stringify(state.anc ?? null),
        ev: window.__ev.join(' '),
        flags: `${el.isDirty() ? 1 : 0}${el.undoStack.length ? 1 : 0}${el.redoStack.length ? 1 : 0}:${el.editsSinceLoad}`,
        ctx: ne ? `note:${ne.kind}:${ne.id}` : (el.cellEdit ? `cell:${el.cellEdit.anchor}:${JSON.stringify(el.cellEdit.hops.map((h) => [h.r, h.c]))}` : 'body'),
        no: `${opts('footnote')}#${opts('endnote')}#${el.noteCount('footnote')}#${el.noteCount('endnote')}`,
        np: JSON.stringify(el.noteSettings),
        ref: el.noteRefs().map((r) => `${r.kind}:${r.id}:${r.para}:${r.cell ? `${r.cell.anchor}.${r.cell.hops[0].r}.${r.cell.hops[0].c}` : '-'}:${r.offset}`).join(','),
        nts: `${noteSig(fd.footnotes)}\n#\n${noteSig(fd.endnotes)}`,
        sect: sects.join('\n'),
      };
    });
    out.set(key, 'paras', digest(revive(st.paras)));
    out.set(key, 'sel', st.sel);
    out.set(key, 'csel', st.csel);
    out.set(key, 'anc', st.anc);
    out.set(key, 'ev', st.ev || '-');
    out.set(key, 'flags', st.flags);
    out.set(key, 'ctx', st.ctx);
    out.set(key, 'no', st.no);
    out.set(key, 'np', st.np);
    out.set(key, 'ref', sha(st.ref));
    out.set(key, 'nts', sha(st.nts));
    out.set(key, 'sect', sha(st.sect));
    if (j % 8 === 0 || j === STEPS) {
      const b64 = await page.evaluate(() => { try { const b = window.__el.saveDocx(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000)); return btoa(s); } catch { return null; } });
      let d = 'err';
      let parts = 'err';
      if (b64) {
        const pkg = DocxPackage.load(Uint8Array.from(Buffer.from(b64, 'base64')));
        d = Digest.of(Buffer.from(pkg.getPartText('word/document.xml') ?? '', 'utf8')).short;
        parts = ['word/footnotes.xml', 'word/endnotes.xml', 'word/settings.xml', 'word/styles.xml', '[Content_Types].xml', 'word/_rels/document.xml.rels']
          .map((p) => `${p}=${Digest.of(Buffer.from(pkg.getPartText(p) ?? '-', 'utf8')).short}`).join(',');
      }
      out.set(key, 'docxml', d);
      out.set(key, 'parts', parts);
      if (process.env.UO_F5_TEXT && b64) {
        // the texts of the parts of a save, to diff against the Rust ones (UO_F5_DUMP) when a digest differs
        const pkg = DocxPackage.load(Uint8Array.from(Buffer.from(b64, 'base64')));
        for (const p of ['word/document.xml', 'word/footnotes.xml', 'word/endnotes.xml', 'word/settings.xml', 'word/styles.xml']) writeFileSync(join(process.env.UO_F5_TEXT, `ts-${j}-${p.split('/').pop()}`), pkg.getPartText(p) ?? '-');
      }
    }
  }
  await page.close();
  n++;
}
await browser.close();
server.close();
out.write(join(crate, 'tests', 'parity', 'd2b-f5-session.capture'));
console.log(`d2b-f5-session: ${n} documents x ${STEPS} steps`);
