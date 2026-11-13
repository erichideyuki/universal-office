// Wave 7, frontier B of the editor switch (record core/docs/modules/uo-bindings.md "Wave 7 — B: fidelity with Word, the
// declared differences and the internal decisions"): text boxes in table cells and in other boxes of
// `<universal-write engine="rust">` against the TS engine in a real headless Chromium, on documents generated here.
//
//   UO_TS_ROOT=<frozen checkout> UO_WASM_PATH=<session_count .wasm> [UO_CHROMIUM=...] [UO_ES_SOAK=20] [UO_ES_ONLY=<scenario regex>]
//   [UO_ES_DUMP=1] node crates/uo-wasm/tests/browser/editor_switch_fidelity.mjs --mode all|boxes|park|drop|memory
//
// The documents (generated in-harness, open fonts only, written to <target>/es-fidelity-run/docs):
//   cell-lic1   a 2×2 table, an anchored wps text box with text in cell (0,0), layoutInCell="1"
//   cell-lic0   the same with layoutInCell="0"
//   cell-nested a box in a cell of a table nested in a cell
//   box-in-box  a text box whose w:txbxContent holds a second anchored text box (Word's COM Documents.Open fails on it:
//               RPC 0x800706BE, record "Wave 7 — B"; saved as uo-docx-layout/tests/fidelity/box-in-box.docx)
//   linked      two linked boxes (wps:txbx id + wps:linkedTxbx id/seq), both anchored in one cell
//   hf-cell     a box in a cell of a table in the default header
//   note-cell   footnote 1 = a 2-cell table then a paragraph (note-plain: two paragraphs, the control)
//   box-table   a body box whose content is a 2-cell table then a paragraph
// `boxes`: per document the TS element first, then the Rust one, the SAME script (click into the box, type, Enter,
// Backspace, Shift+Arrow, Ctrl+B, Ctrl+Z, Esc, click again, Tab, save; real CDP mouse and keyboard events at points
// computed in the TS run); the facets of editor_switch_stories.mjs per step and at the end (page count and sizes,
// thumbnails, the screen, the saved .docx part by part). Every difference not covered by DECLARED fails, and so does any
// page error. `memory` (count build): the cell-lic1 box script UO_ES_SOAK times (default 20): sessions/engines/handles 0
// after each remove, live bytes one value after warm-up, memory.buffer flat.
// Declared duplicate: copy-adapted from editor_switch_stories.mjs (its page helpers, run loop and memory mode; the target
// `textbox` takes `scope` here: any box, also in cells and headers).
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { writeFileSync, mkdirSync } from 'node:fs';
import { setup, core } from './editor_switch_lib.mjs';

const root = process.env.UO_TS_ROOT;
if (!root) throw new Error('set UO_TS_ROOT to the frozen TS checkout');
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i >= 0 ? process.argv[i + 1] : dflt; };
const MODE = arg('--mode', 'all');
const SOAK = Number(process.env.UO_ES_SOAK ?? 20);
const ONLY = process.env.UO_ES_ONLY ? new RegExp(process.env.UO_ES_ONLY) : null;
const target = process.env.CARGO_TARGET_DIR ?? join(core, 'target');
const wasmPath = process.env.UO_WASM_PATH ?? join(target, 'es-fidelity/session_count.wasm');
const outDir = join(target, 'es-fidelity-run');
const { unzipSync, zipSync, strToU8 } = createRequire(join(root, 'write/package.json'))('fflate');

// ── the documents ───────────────────────────────────────────────────────────────────────────────
const W_NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" '
  + 'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" '
  + 'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
  + 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
  + 'xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" '
  + 'xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" '
  + 'xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" mc:Ignorable="w14"';
const XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>';
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
/** a package with styles (Liberation Sans 11 pt, pt-BR), no theme part; `header`: the default header's body XML.
 *  Wave 7c (W, item 2): a settings part in compatibility mode 15 (defaultTabStop 720) unless `settings: false` — Word opens
 *  a package WITHOUT word/settings.xml in mode 12 and the Rust layout follows it there (the TS reads 15), so the scenarios
 *  compare the engines on mode 15 and one scenario keeps the package without settings (declared, Word authority) */
function docx({ body, header = null, settings = true }) {
  const ct = ['<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>',
    '<Default Extension="xml" ContentType="application/xml"/>',
    '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>',
    '<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>'];
  const rels = [`<Relationship Id="rIdS" Type="${REL}/styles" Target="styles.xml"/>`];
  const files = {};
  if (settings) {
    ct.push('<Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>');
    rels.push(`<Relationship Id="rIdSet" Type="${REL}/settings" Target="settings.xml"/>`);
    files['word/settings.xml'] = strToU8(`${XML}<w:settings ${W_NS}><w:defaultTabStop w:val="720"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>`);
  }
  let hdrRef = '';
  if (header !== null) {
    ct.push('<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>');
    rels.push(`<Relationship Id="rIdH1" Type="${REL}/header" Target="header1.xml"/>`);
    files['word/header1.xml'] = strToU8(`${XML}<w:hdr ${W_NS}>${header}</w:hdr>`);
    hdrRef = '<w:headerReference w:type="default" r:id="rIdH1"/>';
  }
  files['[Content_Types].xml'] = strToU8(`${XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">${ct.join('')}</Types>`);
  files['_rels/.rels'] = strToU8(`${XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${REL}/officeDocument" Target="word/document.xml"/></Relationships>`);
  files['word/_rels/document.xml.rels'] = strToU8(`${XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">${rels.join('')}</Relationships>`);
  files['word/styles.xml'] = strToU8(`${XML}<w:styles ${W_NS}><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Liberation Sans" w:hAnsi="Liberation Sans" w:cs="Liberation Sans"/><w:sz w:val="22"/><w:lang w:val="pt-BR"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="0"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/><w:tblPr><w:tblCellMar><w:left w:w="108" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style></w:styles>`);
  files['word/document.xml'] = strToU8(`${XML}<w:document ${W_NS}><w:body>${body}<w:sectPr>${hdrRef}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>`);
  return zipSync(files, { level: 6 });
}
const run = (text) => `<w:r><w:t xml:space="preserve">${text}</w:t></w:r>`;
/** `bytes` with word/footnotes.xml (the separators and footnote 1 = `noteBody`) */
function withNote(bytes, noteBody) {
  const z = unzipSync(bytes);
  const dec = (u) => Buffer.from(u).toString('utf8');
  z['[Content_Types].xml'] = strToU8(dec(z['[Content_Types].xml']).replace('</Types>', '<Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/></Types>'));
  z['word/_rels/document.xml.rels'] = strToU8(dec(z['word/_rels/document.xml.rels']).replace('</Relationships>', `<Relationship Id="rIdF" Type="${REL}/footnotes" Target="footnotes.xml"/></Relationships>`));
  z['word/footnotes.xml'] = strToU8(`${XML}<w:footnotes ${W_NS}><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote><w:footnote w:id="1">${noteBody}</w:footnote></w:footnotes>`);
  return zipSync(z, { level: 6 });
}
const NOTE_REF = '<w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:footnoteReference w:id="1"/></w:r>';
const para = (runs) => `<w:p>${runs}</w:p>`;
/** an anchored wps rectangle; `text` = the txbxContent XML (a box), `txbx` = its wps:txbx attributes, `linked` = a
 *  linkedTxbx `{ id, seq }` (the continuation of a chain: no own content) */
const anchor = ({ id, rh, posH, posV, w, h, text = null, txbx = '', linked = null, lic = 1 }) => `<w:r><w:drawing><wp:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="${rh}" behindDoc="0" locked="0" layoutInCell="${lic}" allowOverlap="1"><wp:simplePos x="0" y="0"/>${posH}${posV}<wp:extent cx="${w}" cy="${h}"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapSquare wrapText="bothSides"/><wp:docPr id="${id}" name="Caixa de Texto ${id}"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:wsp><wps:cNvSpPr txBox="1"/><wps:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="${w}" cy="${h}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill><a:ln w="12700"><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:ln></wps:spPr>${linked ? `<wps:linkedTxbx id="${linked.id}" seq="${linked.seq}"/>` : `<wps:txbx${txbx}><w:txbxContent>${text}</w:txbxContent></wps:txbx>`}<wps:bodyPr rot="0" vert="horz" wrap="square" lIns="91440" tIns="45720" rIns="91440" bIns="45720" anchor="t"><a:noAutofit/></wps:bodyPr></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>`;
const posHAlign = (rel, al) => `<wp:positionH relativeFrom="${rel}"><wp:align>${al}</wp:align></wp:positionH>`;
const posHOff = (rel, off) => `<wp:positionH relativeFrom="${rel}"><wp:posOffset>${off}</wp:posOffset></wp:positionH>`;
const posV = (rel, off) => `<wp:positionV relativeFrom="${rel}"><wp:posOffset>${off}</wp:posOffset></wp:positionV>`;
const BORDERS = '<w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:right w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="auto"/></w:tblBorders>';
const table = (rows, cw = 4252) => `<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/>${BORDERS}</w:tblPr><w:tblGrid>${rows[0].map(() => `<w:gridCol w:w="${cw}"/>`).join('')}</w:tblGrid>${rows.map((r) => `<w:tr>${r.map((c) => `<w:tc><w:tcPr><w:tcW w:w="${cw}" w:type="dxa"/></w:tcPr>${c}</w:tc>`).join('')}</w:tr>`).join('')}</w:tbl>`;
const cellBox = (id, lic, txt = `caixa ${id}`) => anchor({ id, rh: 251659264 + id, posH: posHOff('column', 100000), posV: posV('paragraph', 50000), w: 1500000, h: 500000, text: para(run(txt)), lic });
const LOREM = 'texto de exemplo para preencher a linha com palavras comuns ';
const DOCS = {
  'cell-lic1': () => docx({ body: para(run('antes da tabela')) + table([[para(run('celula 00 ') + cellBox(1, 1)) + para(run('mais texto na celula')), para(run('celula 01'))], [para(run('celula 10')), para(run('celula 11'))]]) + para(run('depois da tabela')) }),
  // wave 7c (W, item 2): cell-lic1 as a package without settings.xml (Word: compatibility mode 12)
  'cell-lic1 nosettings': () => docx({ settings: false, body: para(run('antes da tabela')) + table([[para(run('celula 00 ') + cellBox(1, 1)) + para(run('mais texto na celula')), para(run('celula 01'))], [para(run('celula 10')), para(run('celula 11'))]]) + para(run('depois da tabela')) }),
  'cell-lic0': () => docx({ body: para(run('antes da tabela')) + table([[para(run('celula 00 ') + cellBox(1, 0)) + para(run('mais texto na celula')), para(run('celula 01'))], [para(run('celula 10')), para(run('celula 11'))]]) + para(run('depois da tabela')) }),
  'cell-nested': () => docx({ body: para(run('antes')) + table([[para(run('externa 00')) + table([[para(run('interna 00 ') + cellBox(1, 1)), para(run('interna 01'))]], 1900) + para(''), para(run('externa 01'))]]) + para(run('depois')) }),
  'box-in-box': () => {
    const inner = anchor({ id: 2, rh: 251659270, posH: posHOff('column', 50000), posV: posV('paragraph', 50000), w: 1200000, h: 400000, text: para(run('caixa interna')) });
    const outer = anchor({ id: 1, rh: 251659266, posH: posHAlign('margin', 'center'), posV: posV('paragraph', 0), w: 3000000, h: 1500000, text: para(run('caixa externa ') + inner) });
    return docx({ body: para(run('paragrafo com a caixa ') + outer) + para(run(LOREM.repeat(2))) });
  },
  'linked': () => {
    const first = anchor({ id: 1, rh: 251659265, posH: posHOff('column', 50000), posV: posV('paragraph', 30000), w: 1300000, h: 300000, text: para(run('primeira caixa com um texto longo que continua na segunda caixa encadeada')), txbx: ' id="1"' });
    const second = anchor({ id: 2, rh: 251659266, posH: posHOff('column', 50000), posV: posV('paragraph', 500000), w: 1300000, h: 400000, linked: { id: 1, seq: 1 } });
    return docx({ body: para(run('antes')) + table([[para(run('celula com duas caixas ') + first + second) + para('') + para('') + para(''), para(run('celula 01'))]]) + para(run('depois')) });
  },
  'hf-cell': () => docx({ header: table([[para(run('cabecalho ') + cellBox(1, 1, 'caixa no cabecalho')), para(run('cabecalho 01'))]]) + para(''), body: para(run(LOREM.repeat(3))) }),
  // fix round 2 (B): the flat editing view of notes and boxes that hold a table (session/ops/hf_flat.rs; the scenario of
  // the review, derived from its probe): footnote 1 = a 2-cell table then a paragraph; the control without a table
  'note-cell': () => withNote(docx({ body: para(run('texto com nota') + NOTE_REF) + para(run(LOREM)) }), table([[para(run('nota celula 00')), para(run('nota celula 01'))]], 2000) + para(run('nota depois'))),
  'note-plain': () => withNote(docx({ body: para(run('texto com nota') + NOTE_REF) + para(run(LOREM)) }), para(run('nota um')) + para(run('nota dois'))),
  // a body box whose w:txbxContent is a 2-cell table then a paragraph
  'box-table': () => docx({ body: para(run('paragrafo com a caixa ') + anchor({ id: 1, rh: 251659266, posH: posHAlign('margin', 'center'), posV: posV('paragraph', 0), w: 3600000, h: 1500000, text: table([[para(run('caixa celula 00')), para(run('caixa celula 01'))]], 2400) + para(run('caixa depois')) })) + para(run(LOREM.repeat(2))) }),
};
const docDir = join(outDir, 'docs');
mkdirSync(docDir, { recursive: true });
const extraDocs = {};
for (const [n, mk] of Object.entries(DOCS)) { const p = join(docDir, `${n}.docx`); writeFileSync(p, mk()); extraDocs[`${n}.docx`] = p; }
// wave 7c (S): the park step's picture in crop mode (an inline PNG, versioned with the d3 fixtures)
extraDocs['pictures.docx'] = join(core, 'crates/uo-docx/tests/parity/d3/fixtures/pictures.docx');

// wave 7 (F1): the chrome's command registry caught as editor_switch_completion.mjs does (`bar` scenarios: `{ k: 'cmd' }`)
const ENTRY_EXTRA = `import { CommandRegistry } from '@universal-office/ui';
{ const add0 = CommandRegistry.prototype.add; CommandRegistry.prototype.add = function (...c) { window.__lastReg = this; return add0.apply(this, c); }; }`;
const env = await setup({ root, wasmPath, outDir, stride: 1, only: '^no fixture$', extraDocs, entryExtra: ENTRY_EXTRA });
const fileOf = (n) => env.label(n).replace(/^.*\//, '').replace(/\.docx$/, '');
const docOf = (want) => { for (const n of env.docsByName.keys()) if (fileOf(n) === want) return n; throw new Error(`no document ${want}`); };
let failures = 0;
const fail = (m) => { failures++; console.log(`FAIL ${m}`); };

/** the page-side helpers (evaluated in each new page) */
const helpers = () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const frame = () => new Promise((r) => requestAnimationFrame(() => r()));
  const fnv = (u8) => { let h = 0x811c9dc5; for (let i = 0; i < u8.length; i++) h = Math.imul(h ^ u8[i], 0x01000193); return (h >>> 0).toString(16).padStart(8, '0'); };
  const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-input', 'uw-protected', 'uw-cellmode', 'uw-notemode', 'uw-hfmode', 'uw-tbmode',
    'uw-sort-error', 'uw-blocked', 'uw-styles-change', 'uw-borderpen'];
  /** the TS overlays the Rust shell does not paint (other frontiers' or declared): off on the TS element for the screen */
  const OVERLAYS = ['paintEngineMarks', 'paintBookmarkMarks', 'paintEngineSpelling', 'paintSearchHits', 'paintTableAdorners', 'paintTocControl', 'paintMultiSelection', 'paintEngineInlines'];
  const log = [];
  const canon = (v) => (Array.isArray(v) ? v.map(canon) : v && typeof v === 'object'
    ? Object.fromEntries(Object.entries(v).filter(([, x]) => x !== undefined && x !== null).sort(([a], [b]) => (a < b ? -1 : 1)).map(([k, x]) => [k, canon(x)]))
    : typeof v === 'number' ? +v.toFixed(3) : v);
  const J = (v) => JSON.stringify(canon(v));
  const fetchDoc = async (n) => new Uint8Array(await (await fetch(`/docs/${n}`)).arrayBuffer());
  const PX = 96 / 1440;
  const api = {
    el: null,
    reg: null,
    async make(engine, o = {}) {
      const el = document.createElement('universal-write');
      if (engine === 'rust') el.setAttribute('engine', 'rust');
      el.setAttribute('fonts', '/fonts');
      if (o.topbar) { el.setAttribute('topbar', ''); el.setAttribute('toolbar', 'ribbon'); el.setAttribute('lang', 'en'); }
      el.style.cssText = `display:block;width:1000px;height:${o.topbar ? 950 : 800}px`;
      const ready = new Promise((r) => el.addEventListener('uw-ready', r, { once: true }));
      window.__lastReg = null;
      window.__adds = [];
      // the bars' preferences (remembered table size, recents) start empty on both runs
      if (o.topbar) try { localStorage.clear(); } catch { /* none */ }
      document.body.appendChild(el);
      await ready;
      if (el.rust) el.rust.tsShading = true; // wave 7c (V, fix round 1): the TS field shading/comment look (ED-304: Word's is the default)
      for (const t of EVENTS) el.addEventListener(t, (e) => { log.push(`${t} ${JSON.stringify(e.detail ?? null)}`); });
      api.el = el;
      api.reg = null;
      if (o.topbar) {
        // the chrome is loaded on demand: its registry is the one created after this element
        for (let i = 0; i < 200 && !window.__lastReg; i++) await sleep(25);
        api.reg = window.__lastReg;
        if (!api.reg) throw new Error('no command registry (the chrome did not load)');
      }
      return true;
    },
    async settle() {
      const el = api.el;
      if (el.rust) await el.rust.idle();
      else { await document.fonts.ready; await sleep(300); }
      await frame();
    },
    async load(n, b64) {
      window.__seed(1);
      if (b64) api.el.loadDocx(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)), { protected: false });
      else if (n) api.el.loadDocx(await fetchDoc(n), { protected: false });
      else api.el.newDocument();
      await api.settle();
      api.el.refresh();
      await api.settle();
      api.el.scrollTop = 0;
      await frame();
      log.length = 0;
      return api.el.layoutPageCount();
    },
    remove() { api.el?.remove(); api.el = null; },
    drain() { const out = log.slice(); log.length = 0; return out; },
    focus() { api.el.focusEditor(); },
    /** page twips → client coordinates (the canvas of the page now: scroll and zoom of this run) */
    client(p) {
      const el = api.el;
      const c = el.canvases[p.page];
      if (!c) return null;
      if (p.scroll !== undefined) el.scrollTop = p.scroll;
      const r = c.getBoundingClientRect();
      return { x: r.left + p.x * PX * el.zoom, y: r.top + p.y * PX * el.zoom };
    },
    /** the TS run: page twips of a client point (the page holding it) */
    toPage(x, y) {
      const el = api.el;
      for (let pg = 0; pg < el.canvases.length; pg++) {
        const r = el.canvases[pg].getBoundingClientRect();
        if (y < r.top || y > r.bottom) continue;
        return { page: pg, x: (x - r.left) / (PX * el.zoom), y: (y - r.top) / (PX * el.zoom), scroll: el.scrollTop };
      }
      return null;
    },
    // ── the targets (TS run only: the TS engine's layout) ──
    cellPoint(r, c, nth = 0) {
      const el = api.el;
      const pages = el.eng?.result.pages ?? [];
      let k = 0;
      for (let pg = 0; pg < pages.length; pg++) {
        for (const b of pages[pg].cells ?? []) {
          const m = /^b(\d+)\/r(\d+)\/c(\d+)$/.exec(b.ref);
          if (!m || +m[2] !== r || +m[3] !== c) continue;
          if (k++ < nth) continue;
          return { page: pg, x: b.x + Math.min(b.w / 2, 240), y: b.y + Math.min(b.h / 2, 200) };
        }
      }
      return null;
    },
    /** the header (or footer) zone of page `pg`: the middle of the margin band */
    hfPoint(kind, pg = 0, xf = 0.5) {
      const el = api.el;
      const p = el.eng?.result.pages[pg];
      if (!p) return null;
      const sp = el.sectionOfPage(pg)?.sectPr ?? el.bodyState().model.sectPr;
      return kind === 'header' ? { page: pg, x: p.widthTw * xf, y: sp.marginTopTwips / 3 } : { page: pg, x: p.widthTw * xf, y: p.heightTw - sp.marginBottomTwips / 3 };
    },
    /** the middle of the body text column, a third down page `pg` */
    bodyPoint(pg = 0) {
      const el = api.el;
      const p = el.eng?.result.pages[pg];
      if (!p) return null;
      for (const line of el.eng.lines) if (line.page === pg && line.loc.kind === 'body') return { page: pg, x: line.line.x + Math.min(line.line.width, 2000) / 2, y: line.line.y + line.line.height / 2 };
      return { page: pg, x: p.widthTw / 2, y: p.heightTw / 3 };
    },
    /** the first note line (the notes area) */
    noteLinePoint() {
      const el = api.el;
      for (const line of el.eng?.lines ?? []) if (line.loc.kind === 'note') return { page: line.page, x: line.line.x + 200, y: line.line.y + line.line.height / 2 };
      return null;
    },
    /** the first note reference in the body: the middle of its character */
    noteRefPoint() {
      const el = api.el;
      const ps = el.bodyState?.().model.paragraphs ?? el.model.paragraphs;
      for (let p = 0; p < ps.length; p++) {
        let acc = 0;
        for (const run of ps[p].runs) {
          if (run.noteRef) {
            const a = el.caretRectAt({ para: p, offset: acc }), b = el.caretRectAt({ para: p, offset: acc + run.text.length });
            if (a && b) return api.toPage((a.x + b.x) / 2, a.y + a.h / 2);
          }
          acc += run.text.length;
        }
      }
      return null;
    },
    /** the first text box with content: inside it, near its top-left text */
    textboxPoint(nth = 0, edge = undefined, scope = undefined) {
      const el = api.el;
      const pages = el.eng?.result.pages ?? [];
      let k = 0;
      for (let pg = 0; pg < pages.length; pg++) {
        for (const o of pages[pg].objects ?? []) {
          if (!o.run?.textbox || !o.content || (o.hf && scope !== 'any')) continue;
          // a box of a top-level body paragraph (the session enters no box inside a table cell: S-G2)
          if (scope !== 'any' && !el.model.paragraphs.some((pp) => pp.runs.includes(o.run))) continue;
          if (k++ < nth) continue;
          // `edge`: px outside the left edge (negative: inside), at the middle of the height (the border band)
          if (edge !== undefined) return { page: pg, x: o.x - edge / ((96 / 1440) * el.zoom), y: o.y + o.h / 2 };
          return { page: pg, x: o.x + Math.min(o.w / 2, 400), y: o.y + Math.min(o.h / 2, 300) };
        }
      }
      return null;
    },
    /** the TS caret point of a body position (x + 0.5, the middle of the caret: editor_switch.mjs `clickTarget`) */
    caretPoint(para, offset) {
      const c = api.el.caretRectAt({ para, offset });
      return c ? api.toPage(c.x + 0.5, c.y + c.h / 2) : null;
    },
    templateId(gallery) { return api.el.headerFooterTemplates(gallery)[0]?.id ?? null; },
    async call(m, a) {
      const r = api.el[m](...(a ?? []));
      const v = r && typeof r.then === 'function' ? await r : r;
      await api.settle();
      return v === undefined ? null : J(v);
    },
    /** where the keyboard goes now (a probe step: the focused element through the shadow roots) */
    focusPath() { const out = []; let d = document; while (d && d.activeElement) { const a = d.activeElement; out.push(a.tagName.toLowerCase() + (a.className && typeof a.className === 'string' ? '.' + a.className.split(' ')[0] : '')); d = a.shadowRoot; } const i = api.el.input; let n = i, inert = false; while (n) { if (n.inert) inert = true; n = n.parentNode ?? n.host; } return out.join('>') + (i ? ` ro=${i.readOnly} dis=${i.disabled} inert=${inert} val=${JSON.stringify(i.value)}` : ''); },
    /** text through the proxy's input event (what the browser does on a key: used after the dialogs below) */
    async inputText(t) { const i = api.el.input; i.value = t; i.dispatchEvent(new Event('input', { bubbles: true })); await api.settle(); },
    /** the open dialogs of the element (the bars' dialogs) */
    openDialogs() { return [...api.el.shadowRoot.querySelectorAll('dialog')].filter((d) => d.open); },
    /** a dialog's form: its title, every field (name, value or checked, disabled) and the action buttons */
    formOf(d) {
      const fields = [...d.querySelectorAll('input,select,textarea')].map((i) => [i.name || i.type, (i.type === 'checkbox' || i.type === 'radio') ? `${i.value}:${i.checked}` : i.value, i.disabled ? 'off' : 'on']);
      const actions = [...d.querySelectorAll('[data-action]')].map((b) => b.dataset.action);
      return { title: d.querySelector('.uo-dialog-head')?.textContent.trim() ?? '', fields, actions };
    },
    /**
     * A bar command by id through the chrome's registry (as a button or a menu item: `reg.run`); `dlg` drives the dialog it
     * opens: `set` { field name → value (a radio: the value to check; a checkbox: true/false) }, `action` (default the
     * primary button). The answer: enabled/checked before the run, the dialog's form as opened, `no dialog` when none.
     */
    async cmd(id, dlg) {
      const reg = api.reg;
      if (!reg) return 'no registry';
      const before = { enabled: reg.enabled(id), checked: reg.checked(id), supported: reg.supported(id) };
      let r;
      try { r = reg.run(id); } catch (e) { return J({ ...before, threw: String(e?.message ?? e) }); }
      let form;
      if (dlg) {
        let d = null;
        for (let i = 0; i < 40 && !d; i++) { await sleep(25); d = api.openDialogs().pop() ?? null; }
        if (!d) form = 'no dialog';
        else {
          form = api.formOf(d);
          for (const [n, v] of Object.entries(dlg.set ?? {})) {
            const els = [...d.querySelectorAll(`[name="${n}"]`)];
            if (!els.length) { form.missing = (form.missing ?? []).concat(n); continue; }
            for (const x of els) {
              if (x.type === 'radio') { if (x.value !== String(v)) continue; x.checked = true; }
              else if (x.type === 'checkbox') x.checked = !!v;
              else x.value = String(v);
              x.dispatchEvent(new Event('input', { bubbles: true }));
              x.dispatchEvent(new Event('change', { bubbles: true }));
            }
          }
          const b = dlg.action ? d.querySelector(`[data-action="${dlg.action}"]`) : d.querySelector('.uo-pill-primary');
          if (!b) form.noAction = dlg.action ?? 'primary';
          else b.click();
          for (let i = 0; i < 80 && api.openDialogs().length; i++) await sleep(25);
          if (api.openDialogs().length) { form.leftOpen = api.openDialogs().map((x) => api.formOf(x).title); for (const x of api.openDialogs()) x.close(); }
        }
      }
      if (r && typeof r.then === 'function') await Promise.race([r, sleep(3000)]);
      await sleep(50);
      await api.settle();
      return J({ ...before, ...(form !== undefined ? { form } : {}) });
    },
    async facets() {
      await frame();
      const el = api.el;
      const sel = el.getSelection();
      const c = el.caretRectAt(sel.focus);
      let caret = null;
      if (c) {
        const cs = [...el.shadowRoot.querySelectorAll('canvas.page')];
        const i = cs.findIndex((cv) => { const r = cv.getBoundingClientRect(); return c.y >= r.top - 0.01 && c.y < r.bottom; });
        if (i >= 0) { const r = cs[i].getBoundingClientRect(); caret = { page: i, x: +(c.x - r.left).toFixed(3), y: +(c.y - r.top).toFixed(3), h: +c.h.toFixed(3) }; }
      }
      const st = el.getSelectionState();
      return {
        sel, caret, events: api.drain(), run: J(st.run ?? {}), para: J(st.para ?? {}), table: J(st.table ?? null),
        ctx: J({ cell: el.editingTableCell(), hf: el.editingHeaderFooter(), note: el.editingNote(), tb: el.editingTextbox() }),
        dirty: el.isDirty(),
      };
    },
    /**
     * The screen canvases now (also mid-scenario: a `{ k: 'screen' }` step, so the overlays of a mode — the header/footer
     * bands and the dim layer, Show Document Text off, a selection of cells, the gridlines — are compared in pixels): the
     * caret off, the TS overlays of the other frontiers off, on the Rust side only this frontier's paint hooks.
     */
    async screen() {
      const el = api.el;
      const cs = [...el.shadowRoot.querySelectorAll('canvas.page')];
      el.scrollTop = 0;
      await frame(); await frame();
      el.ensureVisiblePainted();
      window.clearInterval(el.caretTimer);
      el.caretVisible = false;
      if (!el.rust) for (const m of OVERLAYS) el[m] = () => {};
      // the Rust side: only this frontier's paint hooks (the other frontiers paint the overlays the TS has off here)
      const hk = el.rust?.hooks;
      const kept = hk ? { paint: hk.paint.slice(), overlay: hk.overlay.slice() } : null;
      // wave 7 (B): front O's handles too (the TS paintFloatAndCaret is not among the TS overlays turned off: ED-106 was this filter)
      if (hk) for (const k of ['paint', 'overlay']) hk[k].splice(0, hk[k].length, ...kept[k].filter((f) => f.front === 'S' || f.front === 'O'));
      el.paint();
      await frame();
      const screen = cs.map((cv) => (cv.width > 1 ? fnv(new Uint8Array(cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data.buffer)) : '-'));
      const shots = window.__dump ? cs.map((cv) => (cv.width > 1 ? cv.toDataURL('image/png') : null)) : [];
      if (hk) for (const k of ['paint', 'overlay']) hk[k].splice(0, hk[k].length, ...kept[k]);
      return { screen, shots };
    },
    async end() {
      const el = api.el;
      el.refresh();
      await api.settle();
      const cs = [...el.shadowRoot.querySelectorAll('canvas.page')];
      const sizes = cs.map((cv) => `${cv.style.width}x${cv.style.height}`);
      const thumbs = [];
      for (let i = 0; i < cs.length; i++) {
        const t = document.createElement('canvas');
        const ok = el.paintPageThumbnail(i, t, parseFloat(cs[i].style.width));
        thumbs.push(ok ? fnv(new Uint8Array(t.getContext('2d').getImageData(0, 0, t.width, t.height).data.buffer)) : 'none');
        t.width = t.height = 0;
      }
      const { screen, shots } = await api.screen();
      const bytes = el.saveDocx();
      let bin = '';
      for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
      return { pages: cs.length, sizes, thumbs, screen, shots, docx: btoa(bin) };
    },
    stats() { return window.__e.UniversalWriteElement.engineStats(); },
    async soakOnce() {
      await api.make('rust');
      await api.load(null);
      const el = api.el;
      el.focusEditor();
      el.insertTable(2, 2);
      const key = (k, o = {}) => el.input.dispatchEvent(new KeyboardEvent('keydown', { key: k, code: k, bubbles: true, cancelable: true, ...o }));
      const type = (t) => { el.input.value = t; el.input.dispatchEvent(new Event('input', { bubbles: true })); };
      type('a'); key('Tab'); type('b');
      el.enterHeaderFooter('header', 0); type('h'); key('Escape');
      el.insertFootnote(); type('n'); key('Escape');
      el.saveDocx();
      const during = api.stats();
      api.remove();
      await sleep(0);
      return { during, after: api.stats() };
    },
    // the ribbon (chrome mode)
    button(id) {
      const find = (n) => { if (n.dataset?.a === id) return n; for (const c of n.children ?? []) { const f = find(c); if (f) return f; } if (n.shadowRoot) return find(n.shadowRoot); return null; };
      return find(api.el.shadowRoot);
    },
    buttonState(id) { const bt = api.button(id); return bt ? { disabled: !!(bt.disabled || bt.getAttribute?.('aria-disabled') === 'true' || bt.classList?.contains('disabled')) } : null; },
    tabs() {
      const out = [];
      const walk = (n) => { if (n.getAttribute?.('role') === 'tab' && !n.hidden && getComputedStyle(n).display !== 'none') out.push(n.textContent.trim()); for (const c of n.children ?? []) walk(c); if (n.shadowRoot) walk(n.shadowRoot); };
      walk(api.el.shadowRoot);
      return out;
    },
  };
  window.__es = api;
  window.addEventListener('error', (e) => { (window.__stacks ??= []).push(String(e.error?.stack ?? e.message).slice(0, 900)); });
  return true;
};
const ev = (f, a) => env.page.evaluate(f, a);

// ── the scenarios ───────────────────────────────────────────────────────────────────────────────
// A step: { k: 'api', m, a } | { k: 'press', v } | { k: 'type', v } | { k: 'click' | 'dblclick', at } | { k: 'screen' };
// `at` = { t: 'textbox', nth, scope } (scope 'any': every box with content of the TS layout, also in cells, in boxes
// and in headers — the stories harness takes top-level body boxes only) | { t: 'body', page } | { t: 'hf', kind, page }.
/** the box script of the brief: click into the box, type, Enter, Backspace, Shift+Arrow, Ctrl+B, Ctrl+Z, Esc, click it
 *  again, Tab, then the caret context and the screen */
const BOX = (at) => [
  { k: 'click', at }, { k: 'api', m: 'editingTextbox' }, { k: 'type', v: 'ab' },
  { k: 'press', v: 'Enter' }, { k: 'type', v: 'cd' }, { k: 'press', v: 'Backspace' }, { k: 'press', v: 'Shift+ArrowLeft' },
  { k: 'press', v: 'Shift+ArrowLeft' }, { k: 'press', v: 'Control+b' }, { k: 'press', v: 'Control+z' }, { k: 'press', v: 'Escape' },
  { k: 'api', m: 'editingTextbox' }, { k: 'click', at }, { k: 'type', v: 'e' },
  { k: 'press', v: 'Tab' }, { k: 'type', v: 'f' }, { k: 'api', m: 'editingTextbox' }, { k: 'screen' },
];
/** the header box: the header entered first (double click in its band), then the box script */
// a box anchored in a table cell is not in the TS layout's page.objects (the TS hit test finds none): those boxes are
// targeted by their page twips in the layout (measured on the Rust viewer's PDF of the same bytes = the TS layout by the
// layout parity): 20 pt right of the box text's left, 5 pt under its top
const tb = (nth) => ({ t: 'textbox', nth, scope: 'any' });
const at = (x, y) => ({ t: 'pt', page: 0, x, y });
/** the header box: the header entered first (double click in its band, a quarter of the width: clear of the cell edge,
 *  which lies at exactly half the page width), then the box script */
const HF = [{ k: 'dblclick', at: { t: 'hf', kind: 'header', page: 0, xf: 0.25 } }, { k: 'api', m: 'editingHeaderFooter' }, ...BOX(at(2525, 1011))];
/** the note script (the review's probe): click the first note line, type, Enter, type */
const NOTE = [{ k: 'click', at: { t: 'noteLine' } }, { k: 'api', m: 'editingNote' }, { k: 'type', v: 'xy' }, { k: 'press', v: 'Enter' }, { k: 'type', v: 'z' }, { k: 'screen' }];
const SCENARIOS = [
  { name: 'cell-lic1', doc: 'cell-lic1', steps: BOX(at(2525, 1973)) },
  { name: 'cell-lic1 nosettings', doc: 'cell-lic1 nosettings', steps: BOX(at(2525, 1973)) },
  { name: 'cell-lic0', doc: 'cell-lic0', steps: BOX(at(2525, 1973)) },
  { name: 'cell-nested', doc: 'cell-nested', steps: BOX(at(2638, 2236)) },
  { name: 'box-in-box outer', doc: 'box-in-box', steps: BOX(tb(1)) },
  { name: 'box-in-box inner', doc: 'box-in-box', steps: BOX(tb(0)) },
  { name: 'linked first', doc: 'linked', steps: BOX(at(2447, 1940)) },
  { name: 'linked second', doc: 'linked', steps: BOX(at(2447, 2682)) },
  { name: 'hf-cell', doc: 'hf-cell', steps: HF },
  // the header's cell edge: a double click at exactly half the width (the edge between the two cells)
  { name: 'hf-cell edge', doc: 'hf-cell', steps: [{ k: 'dblclick', at: { t: 'hf', kind: 'header', page: 0 } }, { k: 'type', v: 'x' }] },
  { name: 'note-cell', doc: 'note-cell', steps: NOTE },
  { name: 'note-plain', doc: 'note-plain', steps: NOTE },
  // the note script, then Ctrl+Z, typing and Escape (the undo inside a note that holds a table)
  { name: 'note-cell undo', doc: 'note-cell', steps: [...NOTE.slice(0, -1), { k: 'press', v: 'Control+z' }, { k: 'type', v: 'w' }, { k: 'api', m: 'editingNote' }, { k: 'press', v: 'Escape' }, { k: 'screen' }] },
  { name: 'box-table', doc: 'box-table', steps: BOX(tb(0)) },
  // the box script without its Ctrl+Z (the TS undo defect of 'box-table', DECLARED): typing, Enter, Backspace, Escape,
  // click back in, typing, Tab
  { name: 'box-table typing', doc: 'box-table', steps: BOX(tb(0)).filter((s) => !(s.k === 'press' && s.v === 'Control+z')) },
  // wave 7 (F1, the final review's B major): the box selected (a click 3 px outside its left edge), then Edit Text — the
  // API editSelectedTextbox and the bar's shp:editText: the caret at the end of the LAST flat paragraph (after the table,
  // TS 14549 `tb.paras` = flattenBlocks), the typing there
  { name: 'box-table edit text', doc: 'box-table', steps: [{ k: 'click', at: { t: 'textbox', nth: 0, scope: 'any', edge: 3 } }, { k: 'api', m: 'selectedObjectCount' }, { k: 'api', m: 'editSelectedTextbox' }, { k: 'api', m: 'editingTextbox' }, { k: 'type', v: 'Q' }, { k: 'press', v: 'Escape' }, { k: 'screen' }] },
  { name: 'box-table edit text (bar)', doc: 'box-table', bar: true, steps: [{ k: 'click', at: { t: 'textbox', nth: 0, scope: 'any', edge: 3 } }, { k: 'api', m: 'selectedObjectCount' }, { k: 'cmd', id: 'shp:editText' }, { k: 'api', m: 'editingTextbox' }, { k: 'type', v: 'Q' }, { k: 'press', v: 'Escape' }] },
  // wave 7 (F1, the final review's A2 minor): the Read view refuses the cell paste (TS 20485: `locked() || view === 'read'`):
  // pasteCells answers false there in both engines, then pastes again in the Print view
  { name: 'read view paste', doc: 'cell-lic1', steps: [{ k: 'click', at: { t: 'cell', r: 1, c: 1 } }, { k: 'api', m: 'selectTable' }, { k: 'api', m: 'copy' }, { k: 'api', m: 'hasCellsClip' }, { k: 'api', m: 'setView', a: ['read'] }, { k: 'api', m: 'pasteCells', a: ['overwrite'] }, { k: 'api', m: 'pasteCells', a: ['text'] }, { k: 'api', m: 'isDirty' }, { k: 'api', m: 'setView', a: ['print'] }, { k: 'click', at: { t: 'cell', r: 1, c: 1 } }, { k: 'api', m: 'pasteCells', a: ['overwrite'] }] },
].filter((s) => !ONLY || ONLY.test(s.name));

/**
 * The declared differences (scenario name → { from, facets, end, id, why }, as editor_switch_stories.mjs): `from` the first
 * step that may differ, `facets` the step facets ('*' all), `end` the end facets ('*' all). Wave 7 (B): filled from the
 * measured runs only (record "Wave 7 — B", Declarations kept).
 */
const DECLARED = {
  // (ED-106b, a box anchored inside another box: closed in wave 7c (V) — the frame and handles of the inner box in edit come
  // from the layout's page object of that box, `uo_shell_objects` `boxFloat`, as the TS `tbEdit.rectTwips`; Word itself
  // does not open the document: its hidden Documents.Open fails with RPC 0x800706BE, record "Wave 7 — B")
  // (HF-T1, a table in a header: closed in the review fix round — the session edits such a part through its flat paragraph
  // list as the TS, uo-docx session/ops/hf_flat.rs)
  // TS defect, Word agrees with Rust (review fix round 2): Ctrl+Z inside a box that holds a table. The TS undo parses its
  // state from JSON (popHistory → parseState), so the box's `tb.paras` and `tb.blocks` come back as separate copies; the
  // next reconcileBlocks(tb.blocks, tb.paras, true) finds none of the list's paragraphs in the blocks, puts them ALL at the
  // top of the box before the table and leaves each cell an empty paragraph (the TS saved document.xml of this run). Word,
  // the same keys through COM on the same bytes (target/m2-w7b/word/box-undo.ps1): after the Undo the two paragraphs stay
  // in cell (1,1), cell (1,2) keeps its text and "caixa depois" stays after the table, as the Rust session (uo-docx
  // hf_flat.rs keeps the box's view by paragraph identity). Every step before the Ctrl+Z is equal; 'box-table typing'
  // (no undo) is equal on every facet.
  'box-table': { from: 9, facets: ['caret rect', 'selection', 'screen pixels (step)'], end: ['thumbnail pixels', 'screen pixels', 'saved .docx part bytes'], id: 'TS defect, Word agrees with Rust (box undo)', why: 'the TS undo in a box holding a table moves the box paragraphs out of the cells; Word and Rust keep them there' },
  // the same TS defect in a note (review fix round 2): after Ctrl+Z the TS note model and its blocks are separate JSON
  // copies, syncNoteEdit's reconcileBlocks puts every note paragraph before the table and empties the cells (the TS saved
  // footnotes.xml: 'noxy', 'wta celula 00', 'nota celula 01', 'nota depois', then a table of two empty cells). Word, the
  // same edits through COM (target/m2-w7b/word/note-undo.ps1): the text stays in cell (1,1), cell (1,2) keeps its text,
  // 'nota depois' stays after the table — as the Rust session. The texts are equal in both engines; only where they sit.
  // Wave 7c (W): Word's layout rules over the frozen TS (uo-docx-layout word_w7c.rs, the oracle groups nosettings and
  // txbxcell; record uo-bindings.md "Wave 7c — W"). Item 2: a package without settings.xml is mode 12 in Word with 708-tw
  // default tab stops (COM CompatibilityMode 12, DefaultTabStop 35.4 pt); the Tab of step 14 lands on Word's stop.
  'cell-lic1 nosettings': { from: 14, facets: ['caret rect', 'screen pixels (step)'], end: ['thumbnail pixels', 'screen pixels'], id: 'Word manda sobre o TS (item 2, no settings = mode 12)', why: 'Word opens a package without settings.xml in compatibility mode 12 (708-tw tab stops, the pre-2013 table position); the TS reads mode 15' },
  // Item 3: a chain of linked boxes keeps its first lines in the first box (Word: "primeira caixa" in box 1; the TS leaves
  // box 1 empty in mode 15 — oracle txbxcell lc-cell, re-measured baselines 100.82/137.78 pt)
  'linked first': { from: 17, facets: ['screen pixels (step)'], end: ['thumbnail pixels', 'screen pixels'], id: 'Word manda sobre o TS (item 3, linked boxes)', why: 'Word keeps the chain\'s first lines in the first box; the TS widow rule empties it' },
  'linked second': { from: 17, facets: ['screen pixels (step)'], end: ['thumbnail pixels', 'screen pixels'], id: 'Word manda sobre o TS (item 3, linked boxes)', why: 'Word keeps the chain\'s first lines in the first box; the TS widow rule empties it' },
  // Item 3: mode 15 ignores layoutInCell="0" — Word wraps the cell's text around the box (oracle txbxcell lc-cell-lic0); the
  // TS does not, so the scripted clicks land on other text and every step after the first click differs
  'cell-lic0': { from: 0, facets: '*', end: '*', id: 'Word manda sobre o TS (item 3, layoutInCell="0" wrap)', why: 'Word (mode 15) wraps the cell text around a layoutInCell="0" box; the TS lays the text under it' },
  'note-cell undo': { from: 5, facets: ['caret rect', 'screen pixels (step)'], end: ['thumbnail pixels', 'screen pixels', 'saved .docx part bytes'], id: 'TS defect, Word agrees with Rust (note undo)', why: 'the TS undo in a note holding a table moves the note paragraphs out of the cells; Word and Rust keep them there' },
};
const STEP_FACETS = ['selection', 'caret rect', 'uw-* events', 'state run', 'state para', 'state table', 'contexts', 'dirty', 'api answer', 'screen pixels (step)'];
const END_FACETS = ['page count/sizes', 'thumbnail pixels', 'screen pixels', 'saved .docx part names', 'saved .docx part bytes'];
const declaredFor = (name, facet, step) => {
  const d = DECLARED[name];
  if (!d) return null;
  if (step === undefined) return END_FACETS.includes(facet) && (d.end === '*' || (d.end ?? []).includes(facet)) ? d : null;
  // `steps`: exactly these steps; `from`: every step from there
  const at = d.steps ? d.steps.includes(step) : d.from !== undefined && step >= d.from;
  return at && (d.facets === '*' || (d.facets ?? []).includes(facet)) ? d : null;
};

/** a target resolved in the TS run (page twips + the scroll), or a literal */
async function resolve(t) {
  if (!t || typeof t !== 'object' || !t.t) return t;
  if (t.t === 'template') return ev((g) => window.__es.templateId(g), t.gallery);
  const p = await ev((x) => {
    const a = window.__es;
    const el = a.el;
    const pt = x.t === 'pt' ? { page: x.page ?? 0, x: x.x, y: x.y } : x.t === 'cell' ? a.cellPoint(x.r, x.c, x.nth ?? 0) : x.t === 'hf' ? a.hfPoint(x.kind, x.page ?? 0, x.xf) : x.t === 'body' ? a.bodyPoint(x.page ?? 0)
      : x.t === 'noteLine' ? a.noteLinePoint() : x.t === 'noteRef' ? a.noteRefPoint() : x.t === 'textbox' ? a.textboxPoint(x.nth ?? 0, x.edge, x.scope)
      : x.t === 'caret' ? a.caretPoint(x.para, x.offset) : null;
    if (!pt) return null;
    // the page in view (the same scroll on both runs)
    const c = el.canvases[pt.page];
    if (pt.scroll === undefined && c) {
      const box = el.getBoundingClientRect(), r = c.getBoundingClientRect();
      const y = r.top + pt.y * (96 / 1440) * el.zoom;
      if (y < box.top + 20 || y > box.bottom - 20) el.scrollTop += y - (box.top + box.height / 2);
      pt.scroll = el.scrollTop;
    }
    return pt;
  }, t);
  return p;
}

let cdp = null;
async function runStep(step) {
  const page = env.page;
  const pt = async (p) => (p ? ev((x) => window.__es.client(x), p) : null);
  if (step.k === 'press') { await page.keyboard.press(step.v); return null; }
  if (step.k === 'type') { await page.keyboard.type(step.v); return null; }
  if (step.k === 'api') return ev(([m, a]) => window.__es.call(m, a), [step.m, step.args ?? step.a ?? []]);
  if (step.k === 'input') { await ev((t) => window.__es.inputText(t), step.v); return null; }
  if (step.k === 'probe') return ev(() => window.__es.focusPath());
  if (step.k === 'screen') return ev(() => window.__es.screen());
  if (step.k === 'cmd') return ev(([id, d]) => window.__es.cmd(id, d), [step.id, step.dlg ?? null]);
  const a = await pt(step.p);
  if (!a) return 'no target';
  if (step.k === 'click') await page.mouse.click(a.x, a.y);
  else if (step.k === 'dblclick') await page.mouse.dblclick(a.x, a.y);
  else if (step.k === 'shiftclick') { await page.keyboard.down('Shift'); await page.mouse.click(a.x, a.y); await page.keyboard.up('Shift'); }
  else if (step.k === 'drag') {
    const b = await pt(step.p2);
    if (!b) return 'no target';
    await page.mouse.move(a.x, a.y); await page.mouse.down(); await page.mouse.move(b.x, b.y, { steps: 6 }); await page.mouse.up();
  }
  return null;
}

// ── stories ─────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'boxes' || MODE === 'all') {
  const tally = {};
  const used = new Set();
  const undeclared = [];
  let name = '';
  let own = {};
  const count = (facet, same, where, step) => {
    const t = (tally[facet] ??= { same: 0, diff: 0, declared: 0, first: [] });
    if (same) { t.same++; return; }
    t.diff++;
    own[facet] = (own[facet] ?? 0) + 1;
    const d = declaredFor(name, facet, step);
    if (d) { t.declared++; used.add(name); return; }
    if (t.first.length < 8) t.first.push(where);
    undeclared.push(`${facet}: ${where}`);
  };
  let k = 0;
  for (const sc of SCENARIOS) {
    if (k++ % 4 === 0) {
      await env.openPage(); await env.page.evaluate(helpers);
      cdp = await env.page.context().newCDPSession(env.page);
      if (process.env.UO_ES_DUMP) await ev(() => { window.__dump = true; });
    }
    name = sc.name;
    own = {};
    const errs0 = env.errors.length;
    const docName = sc.doc ? docOf(sc.doc) : null;
    const mk = { topbar: !!sc.bar };
    // `prep`: steps run on a TS element, saved; both runs then load the SAME bytes (a document built in-harness)
    let bytes = null;
    if (sc.prep) {
      await ev(() => window.__es.make('ts'));
      await ev((x) => window.__es.load(x), docName);
      await ev(() => window.__es.focus());
      for (const s of sc.prep) {
        const step = { ...s };
        if (s.at) step.p = await resolve(s.at);
        if (s.a) step.args = await Promise.all(s.a.map(resolve));
        await runStep(step);
      }
      bytes = (await ev(() => window.__es.end())).docx;
      await ev(() => window.__es.remove());
    }
    // TS: resolve the targets while running
    await ev((o) => window.__es.make('ts', o), mk);
    const pagesTs = await ev(([x, b]) => window.__es.load(x, b), [docName, bytes]);
    await ev(() => window.__es.focus());
    const script = [];
    const ts = [];
    for (const s of sc.steps) {
      const step = { ...s };
      if (s.at) step.p = await resolve(s.at);
      if (s.to) step.p2 = await resolve(s.to);
      if (s.a) step.args = await Promise.all(s.a.map(resolve));
      script.push(step);
      const answer = await runStep(step);
      ts.push({ ...(await ev(() => window.__es.facets())), answer });
    }
    const tsEnd = await ev(() => window.__es.end());
    await ev(() => window.__es.remove());
    // Rust: the same steps
    await ev((o) => window.__es.make('rust', o), mk);
    const pagesRs = await ev(([x, b]) => window.__es.load(x, b), [docName, bytes]);
    await ev(() => window.__es.focus());
    const rs = [];
    for (const step of script) {
      const answer = await runStep(step);
      rs.push({ ...(await ev(() => window.__es.facets())), answer });
    }
    const rsEnd = await ev(() => window.__es.end());
    await ev(() => window.__es.remove());
    if (process.env.UO_ES_TRACE) {
      for (let i = 0; i < script.length; i++) {
        const a = ts[i], z = rs[i];
        console.log(`    ${i} ${script[i].k} ${script[i].v ?? script[i].m ?? script[i].id ?? ''} | ts ${JSON.stringify(a.sel.focus)} ${a.ctx} ${a.events.join(';')} | rust ${JSON.stringify(z.sel.focus)} ${z.ctx} ${z.events.join(';')}${a.answer ? ` | answer ${String(a.answer).slice(0, 400)}` : ''}`);
        if (a.table !== z.table) console.log(`      table ts   ${a.table}
      table rust ${z.table}`);
      }
    }
    count('pages after load', pagesTs === pagesRs, `${name}: ${pagesTs} vs ${pagesRs}`);
    for (let i = 0; i < script.length; i++) {
      const a = ts[i], z = rs[i], at = `${name} step ${i} (${script[i].k} ${script[i].v ?? script[i].m ?? script[i].id ?? ''})`;
      count('selection', JSON.stringify(a.sel) === JSON.stringify(z.sel), `${at}: ${JSON.stringify(a.sel)} vs ${JSON.stringify(z.sel)}`, i);
      count('caret rect', JSON.stringify(a.caret) === JSON.stringify(z.caret), `${at}: ${JSON.stringify(a.caret)} vs ${JSON.stringify(z.caret)}`, i);
      count('uw-* events', JSON.stringify(a.events) === JSON.stringify(z.events), `${at}: ${JSON.stringify(a.events)} vs ${JSON.stringify(z.events)}`.slice(0, 700), i);
      count('state run', a.run === z.run, `${at}: ${a.run} vs ${z.run}`.slice(0, 500), i);
      count('state para', a.para === z.para, `${at}: ${a.para} vs ${z.para}`.slice(0, 500), i);
      count('state table', a.table === z.table, `${at}: ${a.table} vs ${z.table}`.slice(0, 900), i);
      count('contexts', a.ctx === z.ctx, `${at}: ${a.ctx} vs ${z.ctx}`, i);
      count('dirty', a.dirty === z.dirty, `${at}: ${a.dirty} vs ${z.dirty}`, i);
      if (script[i].k === 'screen') {
        // a mid-scenario screen: per page, where both engines allocated the canvas (as the end screens)
        const sa = a.answer.screen, sz = z.answer.screen;
        for (let p = 0; p < Math.max(sa.length, sz.length); p++) {
          if (sa[p] === '-' && sz[p] === '-') continue;
          if (sa[p] === '-' || sz[p] === '-' || sa[p] === undefined || sz[p] === undefined) { (tally['screen page allocated (informative)'] ??= { same: 0, diff: 0, declared: 0, first: [] }).diff++; continue; }
          count('screen pixels (step)', sa[p] === sz[p], `${at} page ${p}`, i);
          if (sa[p] !== sz[p] && process.env.UO_ES_DUMP) for (const [e, r] of [['ts', a.answer], ['rust', z.answer]]) if (r.shots[p]) writeFileSync(join(outDir, `screen-${name.replace(/[^A-Za-z0-9]+/g, '_')}-s${i}-p${p}-${e}.png`), Buffer.from(r.shots[p].split(',')[1], 'base64'));
        }
        a.answer = z.answer = null;
      } else count('api answer', a.answer === z.answer, `${at}: ${a.answer} vs ${z.answer}`.slice(0, 700), i);
    }
    count('page count/sizes', tsEnd.pages === rsEnd.pages && JSON.stringify(tsEnd.sizes) === JSON.stringify(rsEnd.sizes), `${name}: ${tsEnd.pages} vs ${rsEnd.pages}`);
    for (let i = 0; i < Math.max(tsEnd.thumbs.length, rsEnd.thumbs.length); i++) count('thumbnail pixels', tsEnd.thumbs[i] === rsEnd.thumbs[i], `${name} page ${i}`);
    for (let i = 0; i < Math.max(tsEnd.screen.length, rsEnd.screen.length); i++) {
      if (tsEnd.screen[i] === '-' && rsEnd.screen[i] === '-') continue;
      // a page one engine's scroll virtualization did not allocate (the TS keeps far buffers the shell's rebuildCanvases
      // released: wave 4 "Process metrics"): informative, the pixels are compared where both painted
      if (tsEnd.screen[i] === '-' || rsEnd.screen[i] === '-') { (tally['screen page allocated (informative)'] ??= { same: 0, diff: 0, declared: 0, first: [] }).diff++; continue; }
      count('screen pixels', tsEnd.screen[i] === rsEnd.screen[i], `${name} page ${i}`);
      if (tsEnd.screen[i] !== rsEnd.screen[i] && process.env.UO_ES_DUMP) for (const [e, r] of [['ts', tsEnd], ['rust', rsEnd]]) if (r.shots[i]) writeFileSync(join(outDir, `screen-${name.replace(/[^A-Za-z0-9]+/g, '_')}-p${i}-${e}.png`), Buffer.from(r.shots[i].split(',')[1], 'base64'));
    }
    const za = unzipSync(Buffer.from(tsEnd.docx, 'base64')), zb = unzipSync(Buffer.from(rsEnd.docx, 'base64'));
    const partsA = Object.keys(za).sort(), partsB = Object.keys(zb).sort();
    count('saved .docx part names', JSON.stringify(partsA) === JSON.stringify(partsB), `${name}: ${partsA.filter((p) => !zb[p]).join(',')} | ${partsB.filter((p) => !za[p]).join(',')}`);
    for (const p of partsA) if (zb[p]) {
      const same = Buffer.compare(Buffer.from(za[p]), Buffer.from(zb[p])) === 0;
      count('saved .docx part bytes', same, `${name} ${p}`);
      if (!same && process.env.UO_ES_DUMP) { writeFileSync(join(outDir, `part-${name.replace(/[^A-Za-z0-9]+/g, '_')}-ts.xml`), Buffer.from(za[p])); writeFileSync(join(outDir, `part-${name.replace(/[^A-Za-z0-9]+/g, '_')}-rust.xml`), Buffer.from(zb[p])); }
    }
    if (env.errors.length !== errs0) console.log(`    stacks: ${JSON.stringify(await ev(() => (window.__stacks ?? []).splice(0)))}`);
    count('page errors', env.errors.length === errs0, `${name}: ${env.errors.slice(errs0).join(' | ')}`);
    let first = '';
    for (let i = 0; i < script.length && !first; i++) {
      for (const f of ['sel', 'caret', 'events', 'run', 'para', 'table', 'ctx', 'dirty', 'answer']) {
        if (JSON.stringify(ts[i][f]) !== JSON.stringify(rs[i][f])) { first = `step ${i} (${script[i].k} ${script[i].v ?? script[i].m ?? script[i].id ?? ''}) ${f}: ${JSON.stringify(ts[i][f])} vs ${JSON.stringify(rs[i][f])}`.slice(0, 500); break; }
      }
    }
    const diffs = Object.entries(own).map(([f, c]) => `${f} ${c}`).join(', ');
    console.log(`  ${name}: ${script.length} steps, pages ${tsEnd.pages}/${rsEnd.pages}${first ? `; first divergence ${first}` : ''}${diffs ? `; differs: ${diffs}` : ''}`);
  }
  console.log(`boxes: ${SCENARIOS.length} scenario(s)`);
  for (const [f, t] of Object.entries(tally)) {
    const tag = t.declared ? ` (declared ${t.declared}, undeclared ${t.diff - t.declared})` : '';
    console.log(`  ${f.padEnd(26)} equal ${t.same}, different ${t.diff}${tag}${t.first.length ? `\n      e.g. ${t.first.join('\n      e.g. ')}` : ''}`);
  }
  console.log(`  declared differences used: ${[...used].map((f) => `${f} (${DECLARED[f].id})`).join(', ') || 'none'}`);
  const stale = Object.keys(DECLARED).filter((f) => SCENARIOS.some((s) => s.name === f) && !used.has(f));
  if (stale.length) fail(`boxes: declared but equal now (stale): ${stale.join(', ')}`);
  for (const u of undeclared) fail(`boxes: undeclared difference: ${u}`);
}

// ── objects (UO_ES_DOC=<generated doc>): the TS layout's page objects (where a box of the document lies) ──
if (MODE === 'objects') {
  await env.openPage(); await env.page.evaluate(helpers);
  await ev(() => window.__es.make('ts'));
  await ev((x) => window.__es.load(x), docOf(process.env.UO_ES_DOC ?? 'cell-lic1'));
  console.log(await ev(() => JSON.stringify((window.__es.el.eng?.result.pages ?? []).map((p) => (p.objects ?? []).map((o) => ({ x: o.x, y: o.y, w: o.w, h: o.h, tb: !!o.run?.textbox, content: !!o.content, hf: o.hf, keys: Object.keys(o) }))))));
}

// ── park (wave 7, F1; wave 7c, S): a .docx and an .odt across DOM moves (remove + append) ─────────────────────────────
// The TS frees nothing of the document at disconnect (universal-write.ts 1765–1793): the model, the layout, the undo
// history, the selection, the Format Painter, the search results and the crop mode survive a DOM move, and every API works
// while the element is detached. Wave 7c (S): the shell keeps the session, the shell engine, the layout and every front's
// state across the move too (ED-25 retired: a .docx was reopened from its bytes, losing undo, selection and dirty). Both
// engines load the document (the .docx `note-plain`, and its TS .odt export), are edited (a bookmark, a footnote, a text
// box), search 'a' with highlight and turn the Format Painter on (sticky), then are moved in the DOM three times; during the
// first move the detached element is asked (bookmarks, save('auto'), dirty, selection, the painter) and edited (a bookmark
// and its undo). After each move every answer must equal the TS's and the engine's own answers before the moves; after the
// moves, goToSearchResult(0) and two undos. A picture in crop mode is moved once (`pictures.docx`).
if (MODE === 'park' || MODE === 'all') {
  await env.openPage();
  await env.page.evaluate(helpers);
  await ev(() => window.__es.make('ts'));
  await ev((x) => window.__es.load(x), docOf('note-plain'));
  const odt = await ev(() => { const b = window.__es.el.exportOdt(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000)); return btoa(s); });
  await ev(() => window.__es.remove());
  const MOVES = 3;
  const probe = async (kind) => ev(async (kind) => {
    const el = window.__es.el;
    const o = {};
    const t = (k, f) => { try { o[k] = JSON.stringify(f()); } catch (e) { o[k] = `THROW ${e.message}`; } };
    t('all', () => el.listStyles('all').map((x) => [x.id, x.name, x.kind, x.builtIn, x.defined]));
    t('recommended', () => el.listStyles().map((x) => x.id));
    t('fmtNormal', () => el.styleFormatting('Normal'));
    t('bookmarks', () => el.getBookmarks({ hidden: true }).map((b) => [b.name, b.text]));
    t('footnotes', () => el.noteCount('footnote'));
    t('section', () => el.getSection());
    t('dirty', () => el.dirty);
    t('selection', () => el.selection);
    t('painter', () => el.painterActive());
    t('text', () => { const sel = el.selection; el.selectAll(); const s = el.selectedText(); el.setSelection(sel.anchor, sel.focus); return s; });
    const b = await el.save();
    o.autoKind = b[0] === 0x50 && b[1] === 0x4b && new TextDecoder().decode(b.subarray(30, 38)) === 'mimetype' ? 'odt' : 'docx';
    let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000));
    o.saved = btoa(s);
    // the .odt's read-only sheet (a .docx would create the style at every probe)
    if (kind === 'odt') t('create', () => el.createStyle({ name: 'Meu Estilo' }));
    return o;
  }, kind);
  const res = {};
  for (const kind of ['docx', 'odt']) {
    res[kind] = {};
    for (const engine of ['ts', 'rust']) {
      const errs0 = env.errors.length;
      await ev((e) => window.__es.make(e), engine);
      const edits = await ev(async ([kind, o, docName]) => {
        const es = window.__es, el = es.el;
        if (kind === 'odt') {
          el.loadOdt(Uint8Array.from(atob(o), (c) => c.charCodeAt(0)), { protected: false });
          await es.settle(); el.refresh(); await es.settle();
        } else await es.load(docName);
        const log = [];
        el.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 5 });
        log.push(el.insertBookmark('marca1'));
        el.setSelection({ para: 1, offset: 3 });
        const id = el.insertFootnote();
        log.push(id);
        await es.settle();
        log.push(el.goToNoteRef('footnote', id));
        el.setSelection({ para: 2, offset: 0 });
        log.push(el.insertTextbox('simple'));
        await es.settle();
        el.exitTextbox();
        await es.settle();
        // the search results (with their highlight) and the sticky Format Painter, element state the TS keeps
        el.setSelection({ para: 0, offset: 1 });
        log.push((el.search('a', { highlight: true }) ?? []).length);
        await es.settle();
        el.setSelection({ para: 1, offset: 2 }, { para: 1, offset: 4 });
        el.copyFormat(true);
        log.push(el.painterActive());
        await es.settle();
        return JSON.stringify(log);
      }, [kind, odt, docOf('note-plain')]);
      const snaps = [await probe(kind)];
      const stats = [];
      const detached = [];
      for (let m = 0; m < MOVES; m++) {
        // the DOM move: out, then back into the same parent; during the first one the detached element is asked and edited
        const r = await ev(async (first) => {
          const es = window.__es, el = es.el, parent = el.parentNode;
          const st = () => { const s = window.__e.UniversalWriteElement.engineStats(); return s && !s.fake ? { sessions: s.sessions, shellEngines: s.shellEngines, handles: s.handles } : null; };
          const attached = st();
          el.remove();
          await new Promise((q) => setTimeout(q, 0));
          const out = st();
          let d = null;
          if (first) {
            d = {};
            const t = (k, f) => { try { d[k] = JSON.stringify(f()); } catch (e) { d[k] = `THROW ${e.message}`; } };
            t('bookmarks', () => el.getBookmarks({ hidden: true }).map((b) => [b.name, b.text]));
            t('dirty', () => el.isDirty());
            t('selection', () => el.getSelection());
            t('painter', () => el.painterActive());
            try {
              const b = await el.save('auto');
              let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000));
              d.saved = btoa(s);
            } catch (e) { d.saved = `THROW ${e.message}`; }
            el.setSelection({ para: 0, offset: 6 }, { para: 0, offset: 8 });
            t('edit', () => el.insertBookmark('m2'));
            t('editBookmarks', () => el.getBookmarks({ hidden: true }).map((b) => b.name));
            el.undo();
            t('undoBookmarks', () => el.getBookmarks({ hidden: true }).map((b) => b.name));
            t('undoDirty', () => el.isDirty());
            el.setSelection({ para: 1, offset: 2 }, { para: 1, offset: 4 });
          }
          parent.appendChild(el);
          await new Promise((q) => setTimeout(q, 50));
          await es.settle();
          return { attached, out, d };
        }, m === 0);
        stats.push(r);
        if (r.d) detached.push(r.d);
        snaps.push(await probe(kind));
      }
      // after the moves: the search results are still there (the TS goes to the first hit), then the undo history (the TS
      // keeps its stacks): Ctrl+Z takes the text box out, a 2nd the footnote
      const go = await ev(async () => { const es = window.__es, el = es.el; const r = el.goToSearchResult(0); await es.settle(); return JSON.stringify([r, el.selection]); });
      for (let u = 0; u < 2; u++) {
        await ev(async () => { const es = window.__es; es.el.undo(); await es.settle(); });
        snaps.push(await probe(kind));
      }
      res[kind][engine] = { edits, snaps, stats, detached, go, errors: env.errors.length - errs0 };
      await ev(() => window.__es.remove());
    }
  }
  const partsOf = (b64) => { const z = unzipSync(Buffer.from(b64, 'base64')); return Object.fromEntries(Object.keys(z).sort().map((p) => [p, Buffer.from(z[p]).toString('latin1')])); };
  const cmp = (label, a, b) => {
    const st = [];
    for (const k of Object.keys(a)) {
      if (k === 'saved' && !String(a.saved).startsWith('THROW') && !String(b.saved).startsWith('THROW')) {
        const pa = partsOf(a.saved), pb = partsOf(b.saved);
        const bad = [...new Set([...Object.keys(pa), ...Object.keys(pb)])].filter((p) => pa[p] !== pb[p]);
        if (bad.length) { fail(`park: ${label} save('auto') parts ${bad.join(', ')}`); for (const p of bad.slice(0, 2)) { const x = pa[p] ?? '', y = pb[p] ?? ''; let i = 0; while (i < x.length && x[i] === y[i]) i++; console.log(`    ${p} @${i}\n      a …${x.slice(Math.max(0, i - 100), i + 200)}\n      b …${y.slice(Math.max(0, i - 100), i + 200)}`); } }
        st.push(`save('auto') parts ${bad.length ? `DIFFERENT ${bad.length}` : `equal ${Object.keys(pa).length}`}`);
      } else if (a[k] !== b[k]) { fail(`park: ${label} ${k}: ${String(a[k]).slice(0, 300)} vs ${String(b[k]).slice(0, 300)}`); st.push(`${k} DIFFERENT`); } else st.push(`${k} equal`);
    }
    console.log(`park ${label}: ${st.join('; ')}`);
  };
  for (const kind of ['docx', 'odt']) {
    const { ts, rust } = res[kind];
    console.log(`park ${kind}: edits ts ${ts.edits}, rust ${rust.edits}`);
    if (ts.edits !== rust.edits) fail(`park ${kind}: the edits answered ts ${ts.edits} vs rust ${rust.edits}`);
    for (let m = 0; m <= MOVES + 2; m++) {
      cmp(`${kind} ts vs rust ${m > MOVES ? `after the moves, undo ${m - MOVES}` : m ? `after move ${m}` : 'before the moves'}`, ts.snaps[m], rust.snaps[m]);
      if (m && m <= MOVES) cmp(`${kind} rust before the moves vs after move ${m}`, rust.snaps[0], rust.snaps[m]);
    }
    cmp(`${kind} ts vs rust while detached`, ts.detached[0], rust.detached[0]);
    console.log(`park ${kind}: goToSearchResult(0) after the moves ts ${ts.go}, rust ${rust.go}`);
    if (ts.go !== rust.go) fail(`park ${kind}: goToSearchResult(0) after the moves ts ${ts.go} vs rust ${rust.go}`);
    // the step is not vacuous: the edits are there after the last move (the bookmark, the footnote, the page size), the
    // painter is on, the detached edit and its undo ran, and the two undos change the document (the box, then the footnote)
    const z = rust.snaps[MOVES], pg = JSON.parse(z.section);
    console.log(`park ${kind}: rust after move ${MOVES}: bookmarks ${z.bookmarks}, footnotes ${z.footnotes}, dirty ${z.dirty}, painter ${z.painter}, page ${pg?.pageWidthTwips} x ${pg?.pageHeightTwips}, text ${JSON.parse(z.text).length} chars`);
    const u1 = rust.snaps[MOVES + 1], u2 = rust.snaps[MOVES + 2], d = rust.detached[0];
    console.log(`park ${kind}: rust while detached: bookmarks ${d.bookmarks}, painter ${d.painter}, save ${String(d.saved).startsWith('THROW') ? d.saved : 'ok'}, edit ${d.edit} → ${d.editBookmarks}, undo → ${d.undoBookmarks}`);
    console.log(`park ${kind}: rust undo 1: save ${u1.saved === z.saved ? 'unchanged' : 'changed'}, footnotes ${u1.footnotes}; undo 2: footnotes ${u2.footnotes}`);
    const f0 = Number(rust.snaps[0].footnotes);
    if (!z.bookmarks.includes('marca1') || Number(z.footnotes) !== f0 || f0 < 1 || z.painter !== 'true' || u1.saved === z.saved || Number(u2.footnotes) !== f0 - 1) fail(`park ${kind}: the edits, the painter or the undo history did not survive the moves`);
    if (!d.editBookmarks.includes('m2') || d.undoBookmarks.includes('m2') || String(d.saved).startsWith('THROW')) fail(`park ${kind}: the detached element did not answer, edit or save (${JSON.stringify(d).slice(0, 300)})`);
    const kindAfter = rust.snaps[MOVES].autoKind;
    console.log(`park ${kind}: rust while detached ${JSON.stringify(rust.stats.map((s) => s.out))} (attached ${JSON.stringify(rust.stats.map((s) => s.attached))}); kinds ts ${ts.snaps[MOVES].autoKind}, rust ${kindAfter}`);
    if (kindAfter !== kind) fail(`park ${kind}: rust save('auto') after the moves is ${kindAfter}`);
    // while detached: the document is kept whole (the session, its shell engine and the installed layout)
    // (the counts equal the attached ones: an element removed before, not yet collected, may still count; `env.gc()`)
    for (const s of rust.stats) if (!s.out || s.out.sessions !== s.attached.sessions || s.out.shellEngines !== s.attached.shellEngines || s.out.handles !== s.attached.handles || s.out.sessions < 1 || s.out.shellEngines < 1) fail(`park ${kind}: rust while detached ${JSON.stringify(s)}`);
    if (ts.errors || rust.errors) fail(`park ${kind}: page errors ts ${ts.errors}, rust ${rust.errors}`);
  }
  // a picture in crop mode moved in the DOM: the TS keeps the crop (its `cropping` state); the shell's objects front too
  {
    const crop = {};
    // (the click point from the TS run, as the boxes scenarios: the Rust element answers the same page geometry)
    let pt = null;
    for (const engine of ['ts', 'rust']) {
      await ev((e) => window.__es.make(e), engine);
      await ev((x) => window.__es.load(x), docOf('pictures'));
      if (engine === 'ts') pt = await ev(() => { const el = window.__es.el; const h = el.enumInlinePics()[0]; if (!h) return null; const cv = el.canvases[h.page]; const r = cv.getBoundingClientRect(); const s = (96 / 1440) * el.zoom; return { x: r.left + (h.rect.x + h.rect.w / 2) * s, y: r.top + (h.rect.y + h.rect.h / 2) * s }; });
      if (!pt) { fail(`park crop ${engine}: no inline picture`); continue; }
      await env.page.mouse.click(pt.x, pt.y);
      crop[engine] = await ev(async () => {
        const es = window.__es, el = es.el, parent = el.parentNode;
        await es.settle();
        el.startImageCrop();
        await es.settle();
        const before = el.croppingImage();
        el.remove();
        await new Promise((q) => setTimeout(q, 0));
        const detached = el.croppingImage();
        parent.appendChild(el);
        await new Promise((q) => setTimeout(q, 50));
        await es.settle();
        return JSON.stringify({ before, detached, after: el.croppingImage() });
      });
      await ev(() => window.__es.remove());
    }
    console.log(`park crop (pictures.docx): ts ${crop.ts}, rust ${crop.rust}`);
    if (crop.ts !== crop.rust) fail(`park crop: ts ${crop.ts} vs rust ${crop.rust}`);
    else if (!JSON.parse(crop.ts).before) fail('park crop: the picture is not in crop mode (the check is vacuous)');
  }
}

// ── drop (wave 7c, S; F1's probe promoted): an element removed for good frees its document when collected ────────────
// 20 cycles each for a .docx and an .odt: an engine="rust" element, loaded, edited (a bookmark), removed and NEVER
// re-attached, its session, editor and element held only by WeakRefs; after `env.gc()` no session, shell engine or layout
// handle is left, every WeakRef is cleared, and the live bytes are flat from cycle 5.
if (MODE === 'drop' || MODE === 'all') {
  await env.openPage();
  await env.page.evaluate(helpers);
  await ev(() => window.__es.make('ts'));
  await ev((x) => window.__es.load(x), docOf('note-plain'));
  const odt = await ev(() => { const b = window.__es.el.exportOdt(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000)); return btoa(s); });
  await ev(() => window.__es.remove());
  const N = Number(process.env.UO_ES_DROP ?? 20);
  for (const kind of ['docx', 'odt']) {
    const lives = [];
    for (let i = 0; i < N; i++) {
      const r = await ev(async ([kind, o, docName]) => {
        let el = document.createElement('universal-write');
        el.setAttribute('engine', 'rust');
        el.setAttribute('fonts', '/fonts');
        el.style.cssText = 'display:block;width:900px;height:700px';
        const ready = new Promise((q) => el.addEventListener('uw-ready', q, { once: true }));
        document.body.appendChild(el);
        await ready;
        if (kind === 'odt') el.loadOdt(Uint8Array.from(atob(o), (c) => c.charCodeAt(0)), { protected: false });
        else el.loadDocx(new Uint8Array(await (await fetch(`/docs/${docName}`)).arrayBuffer()), { protected: false });
        await el.rust.idle();
        el.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 5 });
        el.insertBookmark('marca1');
        await el.rust.idle();
        const s = window.__e.UniversalWriteElement.engineStats();
        const during = { sessions: s.sessions, shellEngines: s.shellEngines, handles: s.handles };
        window.__drop = [new WeakRef(el.rust.session), new WeakRef(el.rust), new WeakRef(el)];
        el.remove();
        el = null;
        await new Promise((q) => setTimeout(q, 0));
        return { during };
      }, [kind, odt, docOf('note-plain')]);
      const g = await env.gc();
      const alive = await ev(() => (window.__drop ?? []).map((w) => (w.deref() ? 1 : 0)).join(''));
      lives.push(g?.live ?? null);
      if (i === 0 || i === N - 1) console.log(`drop ${kind}: cycle ${i}: during ${JSON.stringify(r.during)}, after gc sessions ${g?.sessions} shellEngines ${g?.shellEngines} handles ${g?.handles} live ${g?.live}; alive (session, editor, element) ${alive}`);
      if (r.during.sessions !== 1 || r.during.shellEngines !== 1) fail(`drop ${kind}: cycle ${i}: while open ${JSON.stringify(r.during)}`);
      if (!g || g.sessions !== 0 || g.shellEngines !== 0 || g.handles !== 0) fail(`drop ${kind}: cycle ${i}: after gc ${JSON.stringify(g)}`);
      if (alive !== '000') fail(`drop ${kind}: cycle ${i}: not collected (session, editor, element) ${alive}`);
    }
    const warm = lives.slice(Math.min(5, lives.length - 1));
    const flat = warm.every((x) => x === warm[0]);
    console.log(`drop ${kind}: ${N} cycles; live bytes after gc from cycle 5 ${flat ? `flat at ${warm[0]}` : `NOT flat: ${warm.join(', ')}`}`);
    if (!flat || !(warm[0] >= 0)) fail(`drop ${kind}: live bytes not flat (or no count build)`);
  }
}

// ── memory ──────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'memory' || MODE === 'all') {
  await env.openPage();
  await env.page.evaluate(helpers);
  // the box point from a TS run, then per cycle: a Rust element, the box clicked, typing, Enter, Ctrl+Z, Esc, save, remove
  const docName = docOf('cell-lic1');
  await ev(() => window.__es.make('ts'));
  await ev((x) => window.__es.load(x), docName);
  const boxAt = await resolve({ t: 'textbox', nth: 0, scope: 'any' });
  await ev(() => window.__es.remove());
  const lives = [], buffers = [];
  let bad = 0;
  for (let i = 0; i < SOAK; i++) {
    await ev(() => window.__es.make('rust'));
    await ev((x) => window.__es.load(x), docName);
    await ev(() => window.__es.focus());
    for (const st of [{ k: 'click', p: boxAt }, { k: 'type', v: 'ab' }, { k: 'press', v: 'Enter' }, { k: 'press', v: 'Control+z' }, { k: 'press', v: 'Escape' }]) await runStep(st);
    const r = await ev(async () => { const a = window.__es; await a.settle(); a.el.saveDocx(); const during = a.stats(); a.remove(); await new Promise((q) => setTimeout(q, 0)); return { during, after: a.stats() }; });
    // wave 7c (S): the removed element keeps its document until it is collected (as the TS element its model): GC first
    if (r.after && !r.after.fake) r.after = await env.gc();
    if (!r.after || r.after.fake) { fail('memory: engineStats() answers nothing (count build?)'); break; }
    // a build without the counters answers live -1: a flat -1 would pass vacuously (UO_WASM_PATH=<the count build>)
    if (!(r.after.live >= 0)) { fail(`memory: no live byte counter (${r.after.live}): run with the count build`); break; }
    if (r.after.sessions !== 0 || r.after.shellEngines !== 0 || r.after.handles !== 0) { bad++; if (bad < 4) fail(`memory: cycle ${i}: after remove ${JSON.stringify(r.after)}`); }
    if (r.during.sessions !== 1) fail(`memory: cycle ${i}: ${r.during.sessions} sessions while open`);
    lives.push(r.after.live);
    buffers.push(r.after.buffer);
  }
  const warm = Math.min(5, lives.length - 1);
  const tail = new Set(lives.slice(warm)), bufTail = new Set(buffers.slice(warm));
  console.log(`memory: ${lives.length} cycles; live bytes after each remove from cycle ${warm}: ${[...tail].join(', ')}; memory.buffer: ${[...bufTail].join(', ')}`);
  if (tail.size !== 1) fail(`memory: live bytes not flat after warm-up (${[...tail].join(', ')})`);
  if (bufTail.size !== 1) fail(`memory: memory.buffer grows (${[...bufTail].join(', ')})`);
}

const pe = env.errors.length;
await env.close();
if (pe) fail(`${pe} page error(s)`);
console.log(failures ? `editor_switch_fidelity: ${failures} failure(s)` : 'editor_switch_fidelity: ok');
process.exit(failures ? 1 : 0);
