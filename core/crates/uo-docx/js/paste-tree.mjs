// The shell half of the paste-tree boundary (D2b-F8, record core/docs/modules/uo-docx-d2b/F8.md
// "The paste-tree boundary"): what `<universal-write>` sends the Rust session for a `text/html`
// clipboard item. The DOM work stays here (DOMParser, the inline CSSOM, `:scope` selectors,
// `innerHTML`); every paste rule runs in Rust over the payload (format v1, `tree.rs`).
//
// This file is the future shell code of the editor switch (queue item 9); the HTML capture uses it
// as the page's encoder. Pure DOM, iterative (no recursion on hostile depth), ESM.

/** The attributes the paste reads (`getAttribute`), in payload order. */
export const ATTRS = ['style', 'class', 'align', 'face', 'color', 'size', 'href', 'src', 'width', 'height'];
/** The inline CSSOM longhands the paste reads (`el.style[x]`), in payload order. */
export const CSS = ['fontWeight', 'fontStyle', 'textDecorationLine', 'textDecoration', 'color', 'fontFamily', 'fontSize', 'verticalAlign', 'textTransform', 'fontVariant', 'fontVariantCaps', 'display', 'textAlign'];

class Enc {
  constructor() { this.buf = new Uint8Array(1024); this.n = 0; }
  grow(k) {
    if (this.n + k <= this.buf.length) return;
    let size = this.buf.length * 2;
    while (size < this.n + k) size *= 2;
    const b = new Uint8Array(size);
    b.set(this.buf.subarray(0, this.n));
    this.buf = b;
  }
  u8(x) { this.grow(1); this.buf[this.n++] = x & 0xff; }
  u16(x) { this.grow(2); this.buf[this.n++] = x & 0xff; this.buf[this.n++] = (x >>> 8) & 0xff; }
  u32(x) { this.grow(4); for (let i = 0; i < 4; i++) this.buf[this.n++] = (x >>> (8 * i)) & 0xff; }
  str(s) {
    this.u32(s.length);
    this.grow(2 * s.length);
    for (let i = 0; i < s.length; i++) { const c = s.charCodeAt(i); this.buf[this.n++] = c & 0xff; this.buf[this.n++] = c >>> 8; }
  }
  bytes() { return this.buf.slice(0, this.n); }
}

const KIND = { 1: 1, 3: 3, 4: 4, 8: 8 };

/** The fragment tree in pre-order, each node with its subtree size (`htmlToParas`: `doc.body` of
 *  `<!doctype html><html><body>${fragmentOf(html)}</body></html>`). */
function fragmentNodes(body) {
  const nodes = [];
  const stack = [body];
  while (stack.length) {
    const x = stack.pop();
    // a number closes the element at that index
    if (typeof x === 'number') { nodes[x].size = nodes.length - x; continue; }
    const i = nodes.length;
    nodes.push({ node: x, size: 1 });
    if (x.nodeType !== 1) continue;
    stack.push(i);
    const kids = [];
    for (let c = x.firstChild; c; c = c.nextSibling) if (KIND[c.nodeType]) kids.push(c);
    for (let k = kids.length - 1; k >= 0; k--) stack.push(kids[k]);
  }
  return nodes;
}

/** The DOM half of `tablesFromHtml` (universal-write.ts:20544), the raw strings only. */
export function tablesDom(html) {
  let doc;
  try { doc = new DOMParser().parseFromString(html, 'text/html'); } catch { return []; }
  const out = [];
  for (const tbl of [...doc.querySelectorAll('table')].filter((x) => !x.parentElement?.closest('table'))) {
    const trs = [...tbl.querySelectorAll(':scope > tr, :scope > thead > tr, :scope > tbody > tr, :scope > tfoot > tr')];
    out.push(trs.map((tr) => [...tr.children].filter((x) => x.tagName === 'TD' || x.tagName === 'TH').map((td) => {
      const blocks = [...td.querySelectorAll('p, div, li')];
      const paras = blocks.length ? blocks.map((b) => b.textContent ?? '') : td.innerHTML.split(/<br\s*\/?>/i).map((h) => { const d = doc.createElement('div'); d.innerHTML = h; return d.textContent ?? ''; });
      return { th: td.tagName === 'TH', colspan: td.getAttribute('colspan'), rowspan: td.getAttribute('rowspan'), paras };
    })));
  }
  return out;
}

/** The payload of a `text/html` clipboard item (format v1). `fragmentOf` is paste-html.ts's. */
export function encodePaste(html, fragmentOf) {
  const e = new Enc();
  for (const c of 'UOPT') e.u8(c.charCodeAt(0));
  e.u16(1); e.u16(0);
  e.str(html);
  const doc = new DOMParser().parseFromString(`<!doctype html><html><body>${fragmentOf(html)}</body></html>`, 'text/html');
  const nodes = fragmentNodes(doc.body);
  e.u32(nodes.length);
  for (const { node, size } of nodes) {
    e.u8(KIND[node.nodeType]); e.u32(size);
    if (node.nodeType !== 1) { e.str(node.nodeValue ?? ''); continue; }
    e.str(node.tagName);
    // an empty value is sent as absent (every read of the paste treats '' as missing)
    const attrs = ATTRS.map((a) => node.getAttribute(a)).map((v) => (v ? v : null));
    let mask = 0; attrs.forEach((v, i) => { if (v !== null) mask |= 1 << i; });
    e.u16(mask); for (const v of attrs) if (v !== null) e.str(v);
    const st = node.style;
    const css = CSS.map((k) => (st && st[k] ? st[k] : null));
    mask = 0; css.forEach((v, i) => { if (v !== null) mask |= 1 << i; });
    e.u16(mask); for (const v of css) if (v !== null) e.str(v);
  }
  const tables = tablesDom(html);
  e.u32(tables.length);
  for (const rows of tables) {
    e.u32(rows.length);
    for (const row of rows) {
      e.u32(row.length);
      for (const c of row) {
        const cs = c.colspan ? c.colspan : null, rs = c.rowspan ? c.rowspan : null;
        e.u8((c.th ? 1 : 0) | (cs !== null ? 2 : 0) | (rs !== null ? 4 : 0));
        if (cs !== null) e.str(cs);
        if (rs !== null) e.str(rs);
        e.u32(c.paras.length);
        for (const p of c.paras) e.str(p);
      }
    }
  }
  return { bytes: e.bytes(), nodes: nodes.length, depth: depthOf(nodes) };
}

/** The deepest nesting of the fragment tree (the root is depth 1). */
function depthOf(nodes) {
  let max = 0;
  const ends = [];
  nodes.forEach((n, i) => {
    while (ends.length && ends[ends.length - 1] <= i) ends.pop();
    ends.push(i + n.size);
    if (ends.length > max) max = ends.length;
  });
  return max;
}
