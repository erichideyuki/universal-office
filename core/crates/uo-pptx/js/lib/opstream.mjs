// Forms and comparison of the canonical op streams (record-ctx.mjs).
//
//   full   everything the recorder emits (property sets, definitions, measureText calls)
//   draw   the comparison form: the lines that decide pixels. Every draw line already carries the resolved state it was
//          made under (`| fs=.. alpha=.. clip=N ...`), so the `set`, `setLineDash`, `gradient`, `stop`, `pattern`, `patternTransform` and
//          `measure` lines are redundant for it and are dropped (the layout's measureText probing alone is most of a full
//          stream). Kept: save/restore/ctm, path lines, fill/stroke/clip, rects, text, images, image data.
//   loose  draw without save/restore/ctm (the geometry is already device-space): two painters that arrange the
//          transform stack differently still compare equal.
//
// Numbers are compared exactly by default; `eps` compares numeric tokens within a tolerance (and everything else
// textually), for the comparison of a Rust-built list against the TS when the decomposition of a transform differs.

const DROP_DRAW = new Set(['set', 'setLineDash', 'gradient', 'stop', 'pattern', 'patternTransform', 'measure']);
const DROP_LOOSE = new Set([...DROP_DRAW, 'save', 'restore', 'ctm']);

/** Lines of a stream (an array of strings, or text). */
export const linesOf = (s) => (typeof s === 'string' ? s.split('\n').filter((l) => l.length) : s);

const opOf = (line) => {
  const t = line.startsWith('@cv') ? line.slice(line.indexOf(' ') + 1) : line;
  const i = t.indexOf(' ');
  return i < 0 ? t : t.slice(0, i);
};

/** Filter a full stream to the `draw` or `loose` form. */
export function form(lines, which = 'draw') {
  if (which === 'full') return linesOf(lines);
  const drop = which === 'loose' ? DROP_LOOSE : DROP_DRAW;
  return linesOf(lines).filter((l) => !drop.has(opOf(l)));
}

const NUM = /^-?\d+(?:\.\d+)?(?:e[+-]?\d+)?$/;

function tokens(line) {
  // split on spaces and the separators inside a token (`,` `(` `)` `:` `;` `|` `=`), keeping the separators; JSON strings whole
  const out = [];
  let i = 0;
  while (i < line.length) {
    const c = line[i];
    if (c === '"') {
      let j = i + 1;
      while (j < line.length && line[j] !== '"') j += line[j] === '\\' ? 2 : 1;
      out.push(line.slice(i, j + 1)); i = j + 1;
    } else if (/[\s,()|;=]/.test(c)) { out.push(c); i++; } else {
      let j = i;
      while (j < line.length && !/[\s,()|;="]/.test(line[j])) j++;
      out.push(line.slice(i, j)); i = j;
    }
  }
  return out;
}

/** Are two lines equal within `eps` on their numeric tokens? */
export function sameLine(x, y, eps = 0) {
  if (x === y) return true;
  if (!eps) return false;
  const a = tokens(x), b = tokens(y);
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] === b[i]) continue;
    if (NUM.test(a[i]) && NUM.test(b[i]) && Math.abs(+a[i] - +b[i]) <= eps * Math.max(1, Math.abs(+a[i]))) continue;
    return false;
  }
  return true;
}

/**
 * Compare two streams: `{ equal, index, a, b, total }` where `index` is the first differing line (0-based) of the
 * filtered streams. `which` selects the form, `eps` the numeric tolerance.
 */
export function compare(streamA, streamB, { which = 'draw', eps = 0 } = {}) {
  const a = form(streamA, which), b = form(streamB, which);
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) if (!sameLine(a[i], b[i], eps)) return { equal: false, index: i, a: a[i], b: b[i], lengths: [a.length, b.length] };
  if (a.length !== b.length) return { equal: false, index: n, a: a[n], b: b[n], lengths: [a.length, b.length] };
  return { equal: true, lengths: [a.length, b.length] };
}

/** `lines:sha1[0..16]` of a form (the digest the captures record). */
export async function digest(lines, which = 'draw') {
  const { createHash } = await import('node:crypto');
  const l = form(lines, which);
  const text = l.length ? l.join('\n') + '\n' : '';
  return { text, lines: l.length, sha: createHash('sha1').update(text).digest('hex').slice(0, 16) };
}

/** Parse an `.ops.gz` text into `[{ deck, slide, view, header, lines }]`. */
export function parseOps(text) {
  const out = [];
  let deck = '', cur = null;
  for (const line of text.split('\n')) {
    if (line.startsWith('#deck ')) { deck = line.slice(6); cur = null; } else if (line.startsWith('#slide ')) {
      const h = Object.fromEntries(line.slice(7).split(' ').slice(2).map((kv) => kv.split('=')));
      const [n, v] = line.slice(7).split(' ');
      cur = { deck, slide: Number(n), view: v.slice(5), header: h, lines: [], error: undefined };
      out.push(cur);
    } else if (line.startsWith('#error ') && cur) cur.error = line.slice(7);
    else if (line.length && cur) cur.lines.push(line);
  }
  return out;
}
