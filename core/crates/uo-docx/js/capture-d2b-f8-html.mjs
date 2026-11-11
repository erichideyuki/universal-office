// Parity capture of the paste-tree boundary and the HTML rules of D2b-F8 (record
// core/docs/modules/uo-docx-d2b/F8.md): in headless Chromium, for every input HTML, the payload the
// shell encoder makes (`js/paste-tree.mjs`), the frozen `htmlToParas` (paste-html.ts, served with
// its types stripped), the gates of `pasteData` (universal-write.ts 5076–5077) and the frozen
// `tablesFromHtml` (the editor bundle's private static, 9,000 twips).
//
//   UO_TS_ROOT=<frozen built checkout> UO_CHROMIUM=<chrome-headless-shell.exe> \
//     node core/crates/uo-docx/js/capture-d2b-f8-html.mjs [--cases N] [--docs N]
//
// Inputs: the Word CF_HTML sample of paste-html.test.ts and browser-like fragments, toHtml exports
// of corpus documents (frozen docx-core, made in Node), a seeded grammar (block/skip/inline tags,
// inline CSS with odd values, Word lists and comments, data: images, tables with spans, hostile
// names, lone surrogates, a 2,000-deep nesting).
//
// Writes tests/parity/d2b-f8-html.capture, per `h#<j>`: `pt` (the payload, base64), `n` (nodes),
// `depth`, `hp` (htmlToParas, canonical: the run props with their key order), `gate`, `tb`.
import { createRequire, stripTypeScriptTypes } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { join, extname, normalize } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter, Rng } from '../../uo-testkit/js/testkit.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const CASES = opt('--cases', 1200);
const DOCS = opt('--docs', 40);

const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const { toHtml } = await ts('export/html.js');

const pwFrom = [join(root, 'slides', 'package.json'), join(root, 'sheet', 'package.json')].find((p) => existsSync(p));
const { chromium } = createRequire(pwFrom)('playwright');

const pasteHtmlJs = stripTypeScriptTypes(readFileSync(join(root, 'write/packages/editor/src/paste-html.ts'), 'utf8'));
const treeJs = readFileSync(join(crate, 'js', 'paste-tree.mjs'), 'utf8');
const MIME = { '.js': 'text/javascript', '.mjs': 'text/javascript', '.html': 'text/html', '.json': 'application/json', '.css': 'text/css', '.ttf': 'font/ttf', '.otf': 'font/otf', '.woff2': 'font/woff2' };
const server = createServer((req, res) => {
  const url = decodeURIComponent((req.url ?? '/').split('?')[0]);
  const send = (type, body) => { res.writeHead(200, { 'content-type': type }); res.end(body); };
  if (url === '/__f8.html') {
    send('text/html', '<!doctype html><meta charset="utf-8"><body><script type="module">import "/write/packages/editor/publish/index.js"; import * as ph from "/__paste-html.js"; import * as pt from "/__paste-tree.js"; window.__ph = ph; window.__pt = pt; window.__ready = true;</script></body>');
    return;
  }
  if (url === '/__paste-html.js') { send('text/javascript', pasteHtmlJs); return; }
  if (url === '/__paste-tree.js') { send('text/javascript', treeJs); return; }
  const file = normalize(join(root, url));
  if (!file.startsWith(normalize(root)) || !existsSync(file)) { res.writeHead(404); res.end(); return; }
  send(MIME[extname(file)] ?? 'application/octet-stream', readFileSync(file));
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

// ── inputs ──
const PNG_B64 = 'iVBORw0KGgoAAAANSUhEUgAAAAIAAAADCAYAAAC56t6BAAAAEklEQVR4nGP4z8DwHxkzEAQAmGsN8wwzWEgAAAAASUVORK5CYII=';
const GIF_B64 = 'R0lGODlhAwACAIAAAP///wAAACwAAAAAAwACAAACAoRRADs=';
const JPG_B64 = '/9j/4AAQSkZJRgABAQEASABIAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////wAALCAABAAEBAREA/8QAFAABAAAAAAAAAAAAAAAAAAAAA//EABQQAQAAAAAAAAAAAAAAAAAAAAD/2gAIAQEAAD8AN//Z';
const WORD = readFileSync(join(root, 'write/packages/editor/src/paste-html.test.ts'), 'utf8').match(/const WORD = `([\s\S]*?)`;/)[1].replace(/\\\\/g, '\\');
const FIXED = [
  WORD,
  WORD.slice(WORD.indexOf('<!--StartFragment-->')),
  '<p>Antes <x-tag data-id="7">marcado</x-tag> depois<video>sem vídeo</video><iframe>x</iframe><button>OK</button><script>alert(1)</script></p>',
  '<p><span style="font-family:Arial;font-size:20pt;color:#ff0000;font-weight:bold">X</span><sup>2</sup></p>',
  '<p>a</p><table><tr><td>1</td><td>2</td></tr></table>',
  '<table><tr><th>H1</th><th colspan="2">H2</th></tr><tr><td rowspan="2">a</td><td>b<br>c</td><td><p>p1</p><p> p2 </p></td></tr><tr><td>d</td><td>e</td></tr></table>',
  '<meta charset="utf-8"><b style="font-weight:normal;" id="docs-internal-guid-1"><p dir="ltr" style="line-height:1.38;margin-top:0pt;margin-bottom:0pt;"><span style="font-size:11pt;font-family:Arial,sans-serif;color:#000000;background-color:transparent;font-weight:700;font-style:normal;font-variant:normal;text-decoration:none;vertical-align:baseline;white-space:pre;white-space:pre-wrap;">Google</span><span style="font-size:11pt;font-family:Arial,sans-serif;color:#000000;font-weight:400;text-decoration:underline;">Docs</span></p><ul><li dir="ltr" style="list-style-type:disc;"><p dir="ltr"><span>item</span></p></li></ul></b>',
  '<ol><li>one<ol><li>sub</li></ol></li><li>two</li></ol><ul><li>x</li></ul>',
  '<pre>line 1\r\nline 2\n\n  indented</pre><code>inline</code>',
  '<h1>T1</h1><h6>T6</h6><center>c</center><div align="right">r</div><p style="text-align:justify">j</p><p align="CENTER">C</p>',
  `<p><img src="data:image/png;base64,${PNG_B64}" width="40"><img src="data:image/gif;base64,${GIF_B64}"><img src="data:image/jpeg;base64,${JPG_B64}" height="9"><img src="data:image/png;base64, ${PNG_B64.slice(0, 10)} ${PNG_B64.slice(10)}"><img src="data:image/png;base64,@@@"><img src="http://x/y.png"></p>`,
  '<a href="https://example.com">link</a> <a href="#top">anchor</a> <a href="JavaScript:alert(1)">js</a> <a href="">empty</a>',
  '<font face="Courier New, monospace" color="red" size="5">f</font><font size="2.5">g</font><font size="9">h</font><font color="constructor">i</font>',
  '<p>a<hr>b</p><table><tr><td>x</td></tr></table><p>after</p>',
  '<ſcript>alert(1)</ſcript><dıv>dotless</dıv><svg><text>svg</text></svg><math><mi>x</mi></math><template><p>t</p></template>',
  '<p>\uD800 lone \uDC00 surrogates 😀</p>',
  '<!--StartFragment--><p>frag</p><!--EndFragment--><p>outside</p>',
  '<body style="display:none"><p>hidden</p></body>',
  `${'<div>'.repeat(2000)}deep${'</div>'.repeat(2000)}`,
  `${'<span>'.repeat(700)}spans${'</span>'.repeat(700)}`,
  '<table><table><tr><td>nested direct</td></tr></table></table><table></table>',
  // (a rowspan of 1e9 hangs the TS: F8-D8, only the Rust no-panic test feeds it)
  '<table><tr><td colspan="1e9">a</td><td colspan="-5" rowspan="99">b</td><td rowspan="1e309">c</td></tr><tr><td>d</td></tr></table>',
  '',
  '   \n\t ',
];

const rng = new Rng(0xf8a);
const pick = (a) => a[rng.below(a.length)];
const BLOCKS = ['p', 'div', 'h1', 'h2', 'h3', 'h4', 'li', 'pre', 'blockquote', 'section', 'article', 'header', 'footer', 'dd', 'dt', 'figcaption', 'address', 'center', 'tr'];
const INLINE = ['b', 'strong', 'i', 'em', 'u', 'ins', 's', 'strike', 'del', 'sub', 'sup', 'code', 'kbd', 'tt', 'span', 'font', 'a', 'cite', 'x-tag', 'small', 'mark'];
const SKIPS = ['script', 'style', 'noscript', 'textarea', 'select', 'button', 'iframe', 'svg', 'template', 'object', 'canvas'];
const CSSV = ['font-weight:bold', 'font-weight:600', 'font-weight:400', 'font-weight:bolder', 'font-weight:normal', 'font-style:italic', 'font-style:oblique 10deg', 'font-style:normal',
  'text-decoration:underline', 'text-decoration:none', 'text-decoration:line-through underline', 'text-decoration-line:line-through', 'color:rgba(1,2,3,0)', 'color:#abc', 'color:rgb(300 20 1)',
  'color:hsl(120 50% 50%)', 'color:transparent', 'color:navy', 'font-family:"Segoe UI",sans-serif', 'font-family:serif', 'font-family:Calibri', 'font-size:999px', 'font-size:1e3pt', 'font-size:10.5pt',
  'font-size:small', 'font-size:0.1pt', 'font-size:2em', 'vertical-align:super', 'vertical-align:sub', 'text-transform:uppercase', 'font-variant:small-caps', 'font-variant-caps:small-caps',
  'display:none', 'display:block', 'text-align:center', 'text-align:end', 'text-align:justify', 'text-align:start', 'mso-list:Ignore', 'mso-list:l0 level2 lfo1', 'mso-list:l1 level1 lfo2',
  'font:bold italic 12pt Arial', 'margin:0'];
const WORDS = ['a', 'Olá', 'mundo', ' ', '  ', '\n', '\t', 'ção', '😀', '1.', '&nbsp;', '&amp;', 'x y', '\uD83D', 'item'];
function attrs() {
  const out = [];
  if (rng.below(3) === 0) out.push(`style="${[...Array(1 + rng.below(3))].map(() => pick(CSSV)).join(';')}"`);
  if (rng.below(8) === 0) out.push(`class="${pick(['MsoTitle', 'MsoSubtitle', 'MsoHeading3', 'MsoHeading8', 'x MsoTitle', 'MsoListParagraph', 'MsoHeading0'])}"`);
  if (rng.below(10) === 0) out.push(`align="${pick(['center', 'right', 'left', 'justify', 'CENTER', ''])}"`);
  return out.length ? ` ${out.join(' ')}` : '';
}
function inline(depth) {
  const k = rng.below(12);
  if (k < 4 || depth > 6) return pick(WORDS);
  if (k === 4) return '<br>';
  if (k === 5) return pick(['<!--[if !supportLists]-->', '<!--[endif]-->', '<![if !supportLists]>', '<![endif]>', '<!-- comment -->']);
  if (k === 6) return `<img src="data:image/${pick(['png', 'PNG', 'jpeg', 'gif'])};base64,${pick([PNG_B64, GIF_B64, JPG_B64, 'abc', ''])}"${rng.below(2) ? ` width="${pick(['10', '0', '-3', 'x', '1e2'])}"` : ''}>`;
  if (k === 7) { const t = pick(SKIPS); return `<${t}>${pick(WORDS)}</${t}>`; }
  const t = pick(INLINE);
  const extra = t === 'a' ? ` href="${pick(['https://e.com', '#x', 'javascript:x', 'mailto:a@b'])}"` : t === 'font' ? ` face="${pick(['Arial', 'serif', '"Times"'])}" size="${pick(['1', '7', '3.5', '0', '8'])}" color="${pick(['red', '#fff', 'rgb(1,2,3)'])}"` : '';
  return `<${t}${extra}${attrs()}>${[...Array(1 + rng.below(3))].map(() => inline(depth + 1)).join('')}</${t}>`;
}
function block(depth) {
  const k = rng.below(10);
  if (k < 2 && depth < 3) {
    const t = pick(['ul', 'ol']);
    return `<${t}>${[...Array(1 + rng.below(3))].map(() => (rng.below(4) === 0 ? block(depth + 1) : `<li${attrs()}>${inline(0)}${rng.below(3) === 0 ? block(depth + 1) : ''}</li>`)).join('')}</${t}>`;
  }
  if (k === 2 && depth < 2) {
    const rows = 1 + rng.below(3);
    return `<table>${[...Array(rows)].map(() => `<tr>${[...Array(1 + rng.below(3))].map(() => `<${pick(['td', 'th'])}${rng.below(4) === 0 ? ` colspan="${pick(['2', '3', '0', '64', 'x'])}"` : ''}${rng.below(5) === 0 ? ` rowspan="${pick(['2', '3', '9'])}"` : ''}>${rng.below(3) === 0 ? block(depth + 1) : inline(0)}</td>`).join('')}</tr>`).join('')}</table>`;
  }
  const t = pick(BLOCKS);
  return `<${t}${attrs()}>${[...Array(1 + rng.below(4))].map(() => inline(0)).join(rng.below(2) ? '' : ' ')}${rng.below(5) === 0 && depth < 3 ? block(depth + 1) : ''}</${t}>`;
}
function gen() {
  const parts = [...Array(1 + rng.below(5))].map(() => (rng.below(4) === 0 ? inline(0) : block(0)));
  let h = parts.join(rng.below(2) ? '\n' : '');
  if (rng.below(6) === 0) h = `<html><head><style>@list l0:level1 {mso-level-number-format:${pick(['bullet', 'decimal', 'image', 'roman-lower'])};}\n@list l1:level2 {}</style></head><body><!--StartFragment-->${h}<!--EndFragment--></body></html>`;
  return h;
}

const inputs = [...FIXED];
for (const { file } of corpus().filter((_, i) => i % 11 === 2).slice(0, DOCS)) {
  try {
    const full = readFullDocument(DocxPackage.load(readBytes(file)), { sources: true });
    const h = toHtml(full, { fullDocument: false });
    if (h.length < 60000) inputs.push(h);
  } catch { /* unreadable */ }
}
while (inputs.length < CASES) inputs.push(gen());

const out = new CaptureWriter([
  'D2b-F8 paste tree and HTML rules in Chromium: the shell encoder (js/paste-tree.mjs), the frozen htmlToParas (paste-html.ts), the pasteData gates, the frozen tablesFromHtml',
  'facets: pt (payload base64), n, depth, hp, gate, tb',
]);
out.meta('browser', `chromium ${browser.version()}`);
out.meta('bundle', createHash('sha256').update(readFileSync(join(root, 'write/packages/editor/publish/index.js'))).digest('hex').slice(0, 32));
out.meta('math-override', 'none (no value of these rules goes through math)');

const page = await context.newPage();
await page.goto(`http://127.0.0.1:${port}/__f8.html`);
await page.waitForFunction(() => window.__ready === true);
const BATCH = 50;
let maxDepth = 0;
for (let b = 0; b < inputs.length; b += BATCH) {
  const batch = inputs.slice(b, b + BATCH);
  const res = await page.evaluate((batch) => {
    const ph = window.__ph, pt = window.__pt;
    const El = customElements.get('universal-write');
    const hx = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
    const b64 = (u8) => { let s = ''; for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000)); return btoa(s); };
    const num = (v) => (v === undefined ? '' : String(v));
    const props = (p) => `${Object.keys(p).join(',')}#${JSON.stringify(p)}`;
    const runs = (rs) => rs.map((r) => `${hx(r.text)}|${hx(props(r.props))}|${r.blockBold ? 1 : 0}|${r.image ? `${r.image.ext},${r.image.data.length},${b64(r.image.data).slice(0, 40)},${num(r.image.wPx)},${num(r.image.hPx)}` : ''}`).join(';');
    return batch.map((html) => {
      const o = {};
      try {
        const e = pt.encodePaste(html, ph.fragmentOf);
        o.pt = b64(e.bytes); o.n = e.nodes; o.depth = e.depth;
      } catch (err) { o.pt = `err:${String(err)}`; }
      try {
        o.hp = ph.htmlToParas(html).map((p) => [p.align ?? '', num(p.heading), p.style ?? '', p.list ? `${p.list.ordered ? 1 : 0},${p.list.level},${p.list.key}` : '', p.pre ? 1 : 0, p.web ? 1 : 0, runs(p.runs)].join('~')).join('\n');
      } catch (err) { o.hp = `err:${String(err)}`; }
      const d = html;
      const rx1 = !/^\s*$/.test(d);
      const rx2 = /<table[\s>]/i.test(d);
      const rx3 = /<(?:p|h[1-6]|li|div)[\s>]/i.test(d.replace(/<table[\s\S]*?<\/table>/gi, ''));
      o.gate = `${rx1 ? 1 : 0}${rx2 ? 1 : 0}${rx3 ? 1 : 0}`;
      try {
        const ts = El.tablesFromHtml(html, 9000);
        o.tb = ts.map((t) => `${t.rows.length}x${t.gridColsTwips.join('.')}=${t.rows.map((row) => row.cells.map((c) => `${c.tcPr?.gridSpan ?? 1}${(c.tcPr?.vMerge ?? '').slice(0, 1)}:${c.paragraphs.map((p) => p.runs.map((r) => hx(r.text) + (r.props.bold ? '*' : '')).join('+')).join('/')}`).join(';')).join('|')}`).join('\n');
      } catch (err) { o.tb = `err:${String(err)}`; }
      return o;
    });
  }, batch);
  res.forEach((o, i) => {
    const key = `h#${b + i}`;
    out.set(key, 'pt', o.pt);
    out.set(key, 'n', String(o.n ?? -1));
    out.set(key, 'depth', String(o.depth ?? -1));
    maxDepth = Math.max(maxDepth, o.depth ?? 0);
    const enc = (s) => Buffer.from(s, 'utf8').toString('base64');
    out.set(key, 'hp', enc(o.hp));
    out.set(key, 'gate', o.gate);
    out.set(key, 'tb', enc(o.tb));
  });
}
out.meta('max-depth', maxDepth);
await browser.close();
server.close();
out.write(join(crate, 'tests', 'parity', 'd2b-f8-html.capture'));
console.log(`d2b-f8-html: ${inputs.length} inputs, deepest fragment tree ${maxDepth}`);
