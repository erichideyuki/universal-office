// Parity capture of D2b-F9's pure model files (record core/docs/modules/uo-docx-d2b/F9.md): the
// frozen editor's `find-engine.ts` (storyText, compileQuery with its RegExp source and V8's
// errors, findAll, foundText, expandReplacement, adaptCase, metaphone, wordForms) and
// `navigation.ts` (findInText, matchSnippet, headingSpan, moveMap, drawingGridOf, gridLines,
// rulerSnap, snapTo) over the body paragraphs of corpus documents, with a large seeded query set:
// plain text with every option, Word specials (^p ^t ^? ^# ^$ ^w ^nnn …), wildcards from a grammar,
// hostile patterns (catastrophic backtracking, V8's endless empty-match loop before an astral
// character, broken groups and classes, huge bounds), sounds-like / all forms, formatting, and
// replacements with ^ codes and \n groups. A findAll the TS does not finish within the timeout is
// captured as `timeout` (the Rust side must answer its step budget there).
//
// The editor's TS files use parameter properties, so Node needs the transform flag:
//
//   UO_TS_ROOT=<built checkout of main> node --experimental-transform-types --no-warnings \
//     core/crates/uo-docx/js/capture-d2b-f9-ops.mjs [--docs N] [--queries N]
//
// Writes tests/parity/d2b-f9-ops.capture. Keys `<doc>#st` (the story text), `<doc>#q<j>` (a query:
// `op`, `cq`, `hits`, `rep`), `<doc>#n<j>` (findInText + matchSnippet), `pure#<name><j>`.
import vm from 'node:vm';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { CaptureWriter, Rng, importFrom } from '../../uo-testkit/js/testkit.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 200);
const QUERIES = opt('--queries', 48);
const TIMEOUT = opt('--timeout', 1500);

const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const fe = await importFrom(root, 'write/packages/editor/src/find-engine.ts');
const nav = await importFrom(root, 'write/packages/editor/src/navigation.ts');
const { LangResolver } = await importFrom(root, 'write/packages/editor/src/spell.ts');

const hx = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
const sha = (t) => createHash('sha256').update(Buffer.from(hx(t))).digest('hex').slice(0, 32);

/** Shared by index with tests/parity_d2b_f9_ops.rs. */
export const FORMATS = [
  undefined, { bold: true }, { italic: true }, { bold: false }, { underline: true }, { vertAlign: 'superscript' }, { vertAlign: null },
  { sizePt: 11 }, { sizePt: 12 }, { color: '000000' }, { highlight: true }, { highlight: false }, { font: 'Calibri' }, { paraStyle: 'Normal' },
  { paraStyle: 'Heading1' }, { charStyle: '' }, { bold: true, italic: true }, { allCaps: false, smallCaps: false, hidden: false }, { paraStyle: 'Ttulo1' },
  { strike: false, doubleStrike: false }, { sizePt: 10.5 }, { vertAlign: 'subscript' }, { color: 'ff0000' }, { font: 'arial' },
];
export const REPLS = ['x', '', 'NOVO', 'a^pb', '^&^&', '\\1-\\2', '^c', '^t^l^m^n^s^-^~^+^=^^', '^0150^u8212^13^11^12^9^65', '\\\\x', '^', '^q', 'Ação ß ŉ', '😀x', '\\', 'ab\\9c^u55357'];
const CLIP = 'clip';
const FLAGS = { c: 'matchCase', w: 'wholeWord', x: 'wildcards', s: 'soundsLike', a: 'allForms', p: 'prefix', f: 'suffix', u: 'ignorePunct', e: 'ignoreSpace' };

const SPECIALS = ['^p', '^t', '^?', '^#', '^$', '^w', '^l', '^m', '^n', '^b', '^s', '^-', '^~', '^+', '^=', '^f', '^e', '^g', '^^', '^0150', '^u8212', '^13', '^12', '^11', '^14', '^30', '^31', '^9', '^047', '^92', '^45', '^0', '^u0', '^1114112', '^55357', '^56832', '^d', '^x', '^', '^u', '^&', '^c', '^99999'];
const HOSTILE_W = ['(a@)@b', '(*)@x', '*a*a*a*a*a*a*z', '(?@)@(?@)@!', '<{99999}', '(<@)', '(@)', '((((((((((a))))))))))', '[😀-😁]', '\\😀@', '[a-😀]', 'x{99999999999999999999}', 'x{3;2}',
  'x{2147483648;2147483647}', '(((a)))@@', '?@?@?@?@?@x', `${'('.repeat(300)}a${')'.repeat(300)}`, '*', '@', '{2}', '[]]', '[!]', '\\', '(', ')', 'a{1,}', '<*>', '(*)\\1', '[a\\]b]', '[\\-]', 'a-b',
  '(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)', '<(*)>', '(?)@(?)@(?)@$', '[!^13]@', '^13', '(^t)@', '*^13*', '{1;}', 'a{0}', '(a{0}){3}', '(<){2}', '<{2}>', '[z-a]', '[!a-', 'x[', '(a', 'a)', '\\(\\)@', '^#', '^?', '^w', '^$', '*e*e*e*e*e*e*e*e*X', '(?@){1;}x', '((?)@)@#'];
const HOSTILE_P = ['e-mail', 'a/b', 'a.b*c+d?', '(x)', '[y]', '{z}', 'a|b', '^w^w', 'a ^w b', 'it\'s', 'ção', 'AÇÃO', 'ſ', 'K', 'ß', 'ẞ', 'ı', 'İ', 'ͅ', '😀', '\uD83D', '\uDE00', 'a­b', '   ', ' a ', 'x\ty', '-', '–', '¶', ''];
const EN_WORDS = ['go', 'went', 'mouse', 'run', 'stop', 'try', 'make', 'Smith', 'night', 'knight', 'phone', 'xylophone', 'wright', 'ghost', 'church', 'judge', 'thomas', 'schedule', 'science', 'aeon', 'whale', 'gnome', 'pneumonia', 'signed', 'sign', 'dumb', 'cities', 'boxes', 'stopped', 'flying', 'better', 'é', "o'neil", 'Cá'];
const POOL = ['a', 'b', 'A', 'ç', 'Ç', ' ', '-', '^', '\\', '[', ']', '(', ')', '{', '}', '?', '*', '@', '<', '>', '!', ';', ',', '1', '9', '😀', '\uD83D', '\uDE00', '\t', 'ß', 'ſ', 'K', '.', "'", '’', 'p', 't', 'w', 'u', '0'];

function wordsOf(text) {
  const out = [];
  for (const m of text.matchAll(/[\p{L}\p{N}'’]+/gu)) if (m[0].length <= 30) out.push(m[0]);
  return out;
}

function pickWord(rng, words) {
  return words.length ? words[rng.below(words.length)] : 'a';
}

function wildPart(rng, words, depth) {
  const k = rng.below(24);
  const w = pickWord(rng, words);
  switch (k) {
    case 0: case 1: case 2: return w.slice(0, 1 + rng.below(3));
    case 3: return '?';
    case 4: return '*';
    case 5: return `${w.slice(0, 1)}@`;
    case 6: return `${w.slice(0, 1)}{${rng.below(3)}}`;
    case 7: return `${w.slice(0, 1)}{${rng.below(2)};${1 + rng.below(3)}}`;
    case 8: return `${w.slice(0, 1)}{${rng.below(3)},}`;
    case 9: return '<';
    case 10: return '>';
    case 11: return ['[a-z]', '[A-Z]', '[0-9]', '[!aeiou]', '[!a-c ]', '[áéíóú]', '[,.;:]', '[ ]'][rng.below(8)];
    case 12: case 13: return depth < 3 ? `(${wildPart(rng, words, depth + 1)}${wildPart(rng, words, depth + 1)})` : 'x';
    case 14: return `\\${POOL[rng.below(POOL.length)]}`;
    case 15: return ['^13', '^t', '^p', '^#', '^-', '^s', '^0150', '^^', '^w'][rng.below(9)];
    case 16: return `[${POOL[rng.below(POOL.length)]}${POOL[rng.below(POOL.length)]}]`;
    case 17: return '@';
    case 18: return `{${rng.below(4)}}`;
    case 19: return POOL[rng.below(POOL.length)];
    default: return w;
  }
}

function genQuery(rng, text, words) {
  const k = rng.below(100);
  let t;
  let f = '';
  const opts = (s) => { for (const c of s) if (rng.below(3) === 0) f += c; };
  if (k < 28) {
    const w = pickWord(rng, words);
    t = rng.below(4) === 0 && text.length > 4 ? text.slice(rng.below(text.length - 3)).slice(0, 2 + rng.below(12)) : w;
    if (rng.below(5) === 0) t = t.toUpperCase();
    opts('cwpfue');
  } else if (k < 38) {
    const sp = SPECIALS[rng.below(SPECIALS.length)];
    const w = pickWord(rng, words).slice(0, 3);
    t = [sp, `${w}${sp}`, `${sp}${w}`, `${sp}${sp}`][rng.below(4)];
    opts('cwue');
  } else if (k < 66) {
    t = '';
    const n = 1 + rng.below(5);
    for (let i = 0; i < n; i++) t += wildPart(rng, words, 0);
    f = 'x';
  } else if (k < 73) {
    t = HOSTILE_W[rng.below(HOSTILE_W.length)];
    f = 'x';
  } else if (k < 78) {
    t = HOSTILE_P[rng.below(HOSTILE_P.length)];
    opts('cwpfue');
  } else if (k < 85) {
    t = rng.below(2) ? pickWord(rng, words) : EN_WORDS[rng.below(EN_WORDS.length)];
    f = rng.below(3) === 0 ? 'sa' : rng.below(2) ? 's' : 'a';
    opts('c');
  } else if (k < 94) {
    t = rng.below(2) ? '' : pickWord(rng, words);
    opts('c');
    return { f, t, fmt: 1 + rng.below(FORMATS.length - 1) };
  } else {
    t = '';
    const n = 1 + rng.below(6);
    for (let i = 0; i < n; i++) t += POOL[rng.below(POOL.length)];
    opts('cwxpfue');
  }
  return { f, t, fmt: rng.below(6) === 0 ? 1 + rng.below(FORMATS.length - 1) : 0 };
}

const queryOf = ({ f, t, fmt }) => {
  const q = { text: t };
  for (const c of f) q[FLAGS[c]] = true;
  if (fmt) q.format = FORMATS[fmt];
  return q;
};

const out = new CaptureWriter([
  'D2b-F9 pure model files: find-engine.ts and navigation.ts of the frozen editor over corpus documents (Node 24.15, --experimental-transform-types)',
  'facets: st (story text), op/cq/hits/rep (queries), fit/snip (navigation search), pure#* (metaphone, wordForms, adaptCase, headingSpan, moveMap, drawingGridOf, gridLines, snapTo)',
]);
out.meta('queries', QUERIES);
out.meta('timeout-ms', TIMEOUT);
const ctx = vm.createContext({ fe });

const files = corpus().filter((_, i) => i % 2 === 0).slice(0, DOCS);
let n = 0;
let timeouts = 0;
for (const { rel, file } of files) {
  let full;
  try { full = readFullDocument(DocxPackage.load(readBytes(file)), { sources: true }); } catch { continue; }
  const paragraphs = [];
  for (const b of full.blocks) if (b.type === 'paragraph') paragraphs.push(b.paragraph);
  if (!paragraphs.length) continue;
  const st = fe.storyText(paragraphs);
  out.set(`${rel}#st`, 'st', `${st.text.length}|${st.starts.length}|${sha(st.text)}`);
  const langs = new LangResolver(full.formatting?.styles);
  const env = { lang: (r, p) => langs.run(r, p).lang, paraStyle: (p) => p.props.styleId ?? p.ppr?.pStyle ?? 'Normal' };
  ctx.st = st;
  ctx.env = env;
  const words = wordsOf(st.text);
  let seed = 0xf9f9;
  for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
  const rng = new Rng(seed);
  for (let j = 0; j < QUERIES; j++) {
    const g = genQuery(rng, st.text, words);
    const key = `${rel}#q${j}`;
    const repl = rng.below(REPLS.length);
    out.set(key, 'op', `${g.f}:${hx(g.t)}:${g.fmt}:${repl}`);
    const q = queryOf(g);
    let cq;
    try {
      const re = fe.compileQuery(q);
      cq = re ? `ok|${re.flags}|${hx(re.source)}` : 'null';
    } catch (e) {
      cq = e instanceof fe.FindError ? `err|${e.code}|${hx(e.message)}` : `err|syntax|${hx(String(e))}`;
    }
    out.set(key, 'cq', cq);
    if (cq.startsWith('err')) continue;
    ctx.q = q;
    let hits;
    let err;
    // a second chance with a longer timeout: polynomial-but-slow searches finish, exponential ones do not
    for (const ms of [TIMEOUT, TIMEOUT * 8]) {
      try { hits = vm.runInContext('fe.findAll(st, q, env)', ctx, { timeout: ms }); err = undefined; break; } catch (e) { err = e; }
    }
    if (err) {
      if (/timed out/.test(String(err))) { out.set(key, 'hits', 'timeout'); timeouts++; continue; }
      out.set(key, 'hits', `throw|${hx(String(err))}`);
      continue;
    }
    const list = hits.map((h) => `${h.start},${h.end}${h.groups ? `,${h.groups.map(hx).join(';')}` : ''}`).join('\n');
    out.set(key, 'hits', `${hits.length}|${sha(list)}`);
    if (process.env.UO_F9_TEXT) out.set(key, 'hits.text', list.replace(/\n/g, ' ') || '-');
    const reps = hits.slice(0, 3).map((h) => {
      const found = fe.foundText(st, h);
      let t = fe.expandReplacement(REPLS[repl], found, h.groups, !!q.wildcards, CLIP);
      if (!q.matchCase && !q.wildcards) t = fe.adaptCase(found, t);
      return hx(t);
    });
    out.set(key, 'rep', reps.join(' ') || '-');
  }
  // the Navigation pane's search (findInText) and its snippets, over some paragraphs
  for (let j = 0; j < 6; j++) {
    const pi = rng.below(paragraphs.length);
    const text = paragraphs[pi].runs.map((r) => r.text).join('');
    const needle = rng.below(4) === 0 ? HOSTILE_P[rng.below(HOSTILE_P.length)] : rng.below(3) === 0 && text.length > 2 ? text.slice(rng.below(text.length - 1)).slice(0, 1 + rng.below(6)) : pickWord(rng, wordsOf(text).length ? wordsOf(text) : words);
    const o = { matchCase: rng.below(2) === 0, wholeWord: rng.below(2) === 0 };
    const key = `${rel}#n${j}`;
    out.set(key, 'op', `${pi}:${hx(needle)}:${o.matchCase ? 1 : 0}${o.wholeWord ? 1 : 0}`);
    const found = nav.findInText(text, needle, o);
    out.set(key, 'fit', found.join(',') || '-');
    out.set(key, 'snip', found.slice(0, 4).map((s) => { const m = nav.matchSnippet(text, s, s + needle.length); return `${hx(m.before)}|${hx(m.match)}|${hx(m.after)}`; }).join(' ') || '-');
  }
  // words of the document: metaphone, wordForms, adaptCase
  for (let j = 0; j < 8; j++) {
    const w = rng.below(3) === 0 ? EN_WORDS[rng.below(EN_WORDS.length)] : pickWord(rng, words);
    const key = `${rel}#w${j}`;
    out.set(key, 'op', hx(w));
    out.set(key, 'mph', fe.metaphone(w) || '-');
    out.set(key, 'forms', [...fe.wordForms(w)].map(hx).join(' '));
    const r = REPLS[rng.below(REPLS.length)];
    out.set(key, 'repl', hx(r));
    out.set(key, 'adapt', hx(fe.adaptCase(w, r)));
  }
  n++;
}

// navigation.ts pure helpers
const prng = new Rng(0x9a71);
for (let j = 0; j < 400; j++) {
  const len = 1 + prng.below(30);
  const levels = Array.from({ length: len }, () => (prng.below(3) === 0 ? 1 + prng.below(9) : 0));
  const i = prng.below(len + 1);
  const [s, e] = nav.headingSpan(levels, i);
  const t = prng.below(len + 1);
  out.set(`pure#hs${j}`, 'op', `${levels.join('.')}:${i}:${t}`);
  out.set(`pure#hs${j}`, 'span', `${s},${e}`);
  out.set(`pure#hs${j}`, 'map', t >= s && t <= e ? '-' : nav.moveMap(len, s, e, t).join('.'));
}
const GRID = ['', '<w:settings/>', '<w:drawingGridHorizontalSpacing w:val="120"/>', '<w:drawingGridHorizontalSpacing w:val="0"/><w:drawingGridVerticalSpacing w:val="-5"/>',
  '<w:displayHorizontalDrawingGridEvery w:val="2"/><w:displayVerticalDrawingGridEvery w:val="0"/>', '<w:doNotUseMarginsForDrawingGridOrigin/><w:drawingGridHorizontalOrigin w:val="1701"/><w:drawingGridVerticalOrigin w:val="1984"/>',
  '<w:doNotUseMarginsForDrawingGridOrigin w:val="0"/>', '<w:doNotUseMarginsForDrawingGridOrigin w:val="false"/>', '<w:doNotUseMarginsForDrawingGridOrigin w:val="off"/>', '<w:doNotUseMarginsForDrawingGridOrigin w:val="1"/>',
  '<w:doNotUseMarginsForDrawingGridOriginX/>', '<w:drawingGridHorizontalSpacingX w:val="5"/><w:drawingGridHorizontalSpacing w:val="7"/>', '<w:drawingGridHorizontalSpacing w:other="1" w:val="9" w:val="11"/>',
  '<w:drawingGridHorizontalSpacing xw:val="3"/>', '<w:drawingGridHorizontalSpacing w:val="12x"/><w:drawingGridHorizontalSpacing w:val="13"/>', '<w:drawingGridHorizontalSpacing w:val="99999999999999999999"/>',
  '<w:drawingGridVerticalSpacing w:val="-0"/>', '<w:drawingGridHorizontalSpacing\n w:val="44"/>', '<w:drawingGridHorizontalSpacing w:val="45">', '<w:drawingGridHorizontalSpacing w:val="4>6"/>',
  '<w:drawingGridHorizontalSpacing w:val=\'7\'/>', '<w:doNotUseMarginsForDrawingGridOrigin w:val="0" w:x="1"/><w:doNotUseMarginsForDrawingGridOrigin/>', '<w:doNotUseMarginsForDrawingGridOrigin foo="w:val=&quot;0&quot;"/>'];
GRID.forEach((x, j) => {
  const g = nav.drawingGridOf(x || undefined);
  out.set(`pure#grid${j}`, 'op', hx(x));
  out.set(`pure#grid${j}`, 'grid', JSON.stringify(g));
  const box = { x0: prng.below(3000) - 500, y0: prng.below(3000), x1: 3000 + prng.below(9000), y1: 4000 + prng.below(9000) };
  const origin = { x: prng.below(2000) - 300, y: prng.below(2000) };
  const lines = nav.gridLines(g, box, origin);
  out.set(`pure#grid${j}`, 'box', `${box.x0},${box.y0},${box.x1},${box.y1},${origin.x},${origin.y}`);
  out.set(`pure#grid${j}`, 'lines', `${lines.xs.length},${lines.ys.length}|${sha(JSON.stringify(lines))}`);
});
for (let j = 0; j < 200; j++) {
  const v = (prng.next() / 4294967296) * 20000 - 5000;
  const unit = prng.below(2) ? 'cm' : 'in';
  out.set(`pure#snap${j}`, 'op', `${v}:${unit}`);
  out.set(`pure#snap${j}`, 'snap', `${nav.rulerSnap(unit)}|${nav.snapTo(v, nav.rulerSnap(unit))}`);
}
out.write(join(crate, 'tests', 'parity', 'd2b-f9-ops.capture'));
console.log(`d2b-f9-ops: ${n} documents x ${QUERIES} queries, ${timeouts} timeouts`);
