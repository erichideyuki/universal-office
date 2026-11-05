// Synthetic documents of the derived-document frontier D5a (export: HTML and Markdown; compare:
// `diffDocuments`): what the oracle corpus has few of. A document is a list of block descriptors
// (paragraphs with runs, tables, lists, images, tracked changes, comments, notes), rendered to a docx
// package; the seeded generator mixes them, and `quirkPairs` derives a second document from the first by
// edits (insert, delete, change and move paragraphs; change text inside runs, split runs; change
// formatting; table rows and cells; images resized or replaced), so the TS and the Rust both read the
// two packages and diff what they read. `capture-export.mjs` and `capture-compare.mjs` run the frozen TS
// over them and write them next to the captures (tests/parity/export.inputs, compare.inputs: the same
// format as full.inputs); the Rust replays read those files, so both sides see the same text.
import { Rng } from '../../uo-testkit/js/testkit.mjs';

export const QUIRK_D5A_SEED = 0xd5a00001;
export const QUIRK_EXPORT_SEEDED = 260;
export const QUIRK_PAIRS_SEEDED = 260;

const NS = [
  'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"',
  'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"',
  'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"',
  'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"',
  'xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"',
].join(' ');
const PROLOG = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n';
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const RELS_NS = 'http://schemas.openxmlformats.org/package/2006/relationships';

const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const rels = (items) => `${PROLOG}<Relationships xmlns="${RELS_NS}">${items.map(([id, type, target, ext]) => `<Relationship Id="${id}" Type="${REL}/${type}" Target="${target}"${ext ? ' TargetMode="External"' : ''}/>`).join('')}</Relationships>`;
const DOC_RELS = rels([
  ['rIdLink', 'hyperlink', 'https://x.test/a?b=1&amp;c=&quot;d&quot;', true],
  ['rIdLink2', 'hyperlink', 'https://y.test/(p)', true],
  ['rIdImg', 'image', 'media/image1.png'],
  ['rIdImg2', 'image', 'media/image2.png'],
  ['rIdNum', 'numbering', 'numbering.xml'],
]);
const NUMBERING = `${PROLOG}<w:numbering ${NS}><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:num w:numId="1001"><w:abstractNumId w:val="1"/></w:num><w:abstractNum w:abstractNumId="2"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="1002"><w:abstractNumId w:val="2"/></w:num></w:numbering>`;

// ───────────────────────── rendering ─────────────────────────

const onoff = (tag, v) => (v === undefined ? '' : v === true ? `<w:${tag}/>` : `<w:${tag} w:val="${v}"/>`);

function rPr(p = {}) {
  let s = '';
  if (p.style) s += `<w:rStyle w:val="${p.style}"/>`;
  if (p.font) s += `<w:rFonts w:ascii="${esc(p.font)}" w:hAnsi="${esc(p.font)}"/>`;
  s += onoff('b', p.b) + onoff('i', p.i) + onoff('caps', p.caps) + onoff('smallCaps', p.smallCaps) + onoff('strike', p.strike) + onoff('dstrike', p.dstrike);
  if (p.color) s += `<w:color w:val="${p.color}"/>`;
  if (p.spacing !== undefined) s += `<w:spacing w:val="${p.spacing}"/>`;
  if (p.sz !== undefined) s += `<w:sz w:val="${p.sz}"/>`;
  if (p.hl) s += `<w:highlight w:val="${esc(p.hl)}"/>`;
  if (p.u) s += `<w:u w:val="${p.u === true ? 'single' : p.u}"/>`;
  if (p.va) s += `<w:vertAlign w:val="${p.va}"/>`;
  return s ? `<w:rPr>${s}</w:rPr>` : '';
}

const IMG = (i, w, h, rid, anchor) => {
  const pic = `<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="${i}" name="p${i}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="${rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="${w}" cy="${h}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>`;
  if (anchor) {
    return `<w:r><w:drawing><wp:anchor distT="0" distB="0" distL="0" distR="0" simplePos="0" relativeHeight="1" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="page"><wp:posOffset>${anchor.x}</wp:posOffset></wp:positionH><wp:positionV relativeFrom="page"><wp:posOffset>${anchor.y}</wp:posOffset></wp:positionV><wp:extent cx="${w}" cy="${h}"/><wp:wrapSquare wrapText="bothSides"/><wp:docPr id="${i}" name="Picture ${i}"/>${pic}</wp:anchor></w:drawing></w:r>`;
  }
  return `<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="${w}" cy="${h}"/><wp:docPr id="${i}" name="Picture ${i}"/>${pic}</wp:inline></w:drawing></w:r>`;
};

/** A run descriptor: `{ t, ...props }`, `{ img }`, `{ note: id }`, `{ field: instr, t }`, plus `ins`/`del`/`cmt`/`link`. */
function run(r, ctx) {
  let inner;
  if (r.raw !== undefined) inner = r.raw;
  else if (r.img) inner = IMG(++ctx.img, r.img.w, r.img.h, r.img.rid ?? 'rIdImg', r.img.anchor);
  else if (r.note !== undefined) inner = `<w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:${r.kind ?? 'footnote'}Reference w:id="${r.note}"/></w:r>`;
  else if (r.field !== undefined) {
    const f = (x) => `<w:r>${rPr(r)}${x}</w:r>`;
    inner = f('<w:fldChar w:fldCharType="begin"/>') + f(`<w:instrText xml:space="preserve"> ${r.field} </w:instrText>`) + f('<w:fldChar w:fldCharType="separate"/>') + `<w:r>${rPr(r)}<w:t xml:space="preserve">${esc(r.t ?? '')}</w:t></w:r>` + f('<w:fldChar w:fldCharType="end"/>');
  } else if (r.del) inner = `<w:r>${rPr(r)}<w:delText xml:space="preserve">${esc(r.t)}</w:delText></w:r>`;
  else if (r.tab) inner = `<w:r>${rPr(r)}<w:tab/></w:r>`;
  else if (r.br) inner = `<w:r>${rPr(r)}<w:br/></w:r>`;
  else inner = `<w:r>${rPr(r)}<w:t xml:space="preserve">${esc(r.t ?? '')}</w:t></w:r>`;
  if (r.del) inner = `<w:del w:id="${++ctx.rev}" w:author="${esc(r.author ?? 'Ann')}" w:date="2024-01-02T03:04:00Z">${inner}</w:del>`;
  if (r.ins) inner = `<w:ins w:id="${++ctx.rev}" w:author="${esc(r.author ?? 'Bob')}" w:date="2024-01-02T03:04:00Z">${inner}</w:ins>`;
  if (r.link) inner = `<w:hyperlink r:id="${r.link}">${inner}</w:hyperlink>`;
  if (r.cmt !== undefined) {
    const ids = [].concat(r.cmt);
    inner = ids.map((id) => `<w:commentRangeStart w:id="${id}"/>`).join('') + inner + ids.map((id) => `<w:commentRangeEnd w:id="${id}"/><w:r><w:commentReference w:id="${id}"/></w:r>`).join('');
  }
  return inner;
}

function pPr(p = {}) {
  let s = '';
  if (p.style) s += `<w:pStyle w:val="${esc(p.style)}"/>`;
  if (p.keepNext) s += '<w:keepNext/>';
  if (p.pageBreakBefore) s += '<w:pageBreakBefore/>';
  if (p.num) s += `<w:numPr><w:ilvl w:val="${p.num.ilvl}"/><w:numId w:val="${p.num.numId ?? 1001}"/></w:numPr>`;
  if (p.borders) s += `<w:pBdr>${Object.entries(p.borders).map(([side, e]) => `<w:${side} w:val="${e.val}" w:sz="${e.sz ?? 4}" w:space="${e.space ?? 1}"${e.color ? ` w:color="${e.color}"` : ''}/>`).join('')}</w:pBdr>`;
  if (p.shd) s += `<w:shd w:val="clear" w:color="auto" w:fill="${p.shd}"/>`;
  if (p.tabs) s += `<w:tabs>${p.tabs.map((t) => `<w:tab w:val="${t.val ?? 'left'}" w:pos="${t.pos}"/>`).join('')}</w:tabs>`;
  if (p.spacing) s += `<w:spacing ${Object.entries(p.spacing).map(([k, v]) => `w:${k}="${v}"`).join(' ')}/>`;
  if (p.ind) s += `<w:ind ${Object.entries(p.ind).map(([k, v]) => `w:${k}="${v}"`).join(' ')}/>`;
  if (p.jc) s += `<w:jc w:val="${p.jc}"/>`;
  if (p.markDel) s += `<w:rPr><w:del w:id="${p.markDel}" w:author="Ann" w:date="2024-01-02T03:04:00Z"/></w:rPr>`;
  return s ? `<w:pPr>${s}</w:pPr>` : '';
}

function para(b, ctx) {
  return `<w:p>${pPr(b.p)}${(b.runs ?? []).map((r) => run(r, ctx)).join('')}</w:p>`;
}

function cell(c, ctx) {
  const pr = `${c.w ? `<w:tcW w:w="${c.w}" w:type="dxa"/>` : ''}${c.span ? `<w:gridSpan w:val="${c.span}"/>` : ''}${c.shd ? `<w:shd w:val="clear" w:color="auto" w:fill="${c.shd}"/>` : ''}`;
  const blocks = (c.blocks ?? [{ type: 'p', runs: [] }]).map((b) => block(b, ctx)).join('');
  return `<w:tc>${pr ? `<w:tcPr>${pr}</w:tcPr>` : ''}${blocks.includes('<w:p>') || blocks.includes('<w:p ') ? blocks : `${blocks}<w:p/>`}</w:tc>`;
}

function table(t, ctx) {
  const grid = (t.grid ?? [2000, 2000, 2000]).map((w) => `<w:gridCol w:w="${w}"/>`).join('');
  return `<w:tbl><w:tblPr>${t.borders ? '<w:tblBorders><w:top w:val="single" w:sz="4" w:color="auto"/></w:tblBorders>' : ''}</w:tblPr><w:tblGrid>${grid}</w:tblGrid>${t.rows.map((r) => `<w:tr>${r.map((c) => cell(c, ctx)).join('')}</w:tr>`).join('')}</w:tbl>`;
}

function block(b, ctx) {
  if (b.type === 'tbl') return table(b, ctx);
  if (b.type === 'raw') return b.xml;
  return para(b, ctx);
}

/** Renders a document descriptor `{ blocks, comments?, footnotes?, endnotes?, images? }` to parts. */
export function render(d) {
  const ctx = { img: 0, rev: 100 };
  const body = d.blocks.map((b) => block(b, ctx)).join('');
  const parts = [
    ['word/document.xml', `${PROLOG}<w:document ${NS}><w:body>${body}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:body></w:document>`],
    ['word/_rels/document.xml.rels', DOC_RELS],
    ['word/numbering.xml', NUMBERING],
    ['word/media/image1.png', d.images?.[0] ?? 'IMG-ONE'],
    ['word/media/image2.png', d.images?.[1] ?? 'IMG-TWO'],
  ];
  const wrapNotes = (kind, list) => `${PROLOG}<w:${kind}s ${NS}><w:${kind} w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:${kind}><w:${kind} w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:${kind}>${list.map((n) => `<w:${kind} w:id="${n.id}">${n.blocks.map((b) => block(b, ctx)).join('')}</w:${kind}>`).join('')}</w:${kind}s>`;
  if (d.footnotes) parts.push(['word/footnotes.xml', wrapNotes('footnote', d.footnotes)]);
  if (d.endnotes) parts.push(['word/endnotes.xml', wrapNotes('endnote', d.endnotes)]);
  if (d.comments) {
    parts.push(['word/comments.xml', `${PROLOG}<w:comments ${NS}>${d.comments.map((c) => `<w:comment w:id="${c.id}"${c.author === undefined ? '' : ` w:author="${esc(c.author)}"`} w:date="2024-01-02T03:04:00Z">${c.paras.map((t) => `<w:p><w:r><w:t xml:space="preserve">${esc(t)}</w:t></w:r></w:p>`).join('')}</w:comment>`).join('')}</w:comments>`]);
  }
  return parts;
}

// ───────────────────────── the seeded generator ─────────────────────────

const WORDS = ['alpha', 'beta', 'gamma', 'Olá', 'mundo', 'a*b', 'x_y', 'p|q', '`code`', '<tag>', '&amp;', 'q"uote"', "it's", '{br}', '[ref]', 'back\\slash', 'é', 'ñ', '日本語', '😀', 'ΑΒΓ', ' nb', 'tab\tx', 'f﻿g', '1.', '-', '#', '>', 'x'];
const STYLES = ['Heading1', 'Heading2', 'Heading3', 'Heading4', 'Heading5', 'Heading6', 'Heading7', 'Heading 3', 'heading2', 'Título1', 'Título2', 'TÍTULO3', 'titulo4', 'Title', 'Título', 'h1', 'H3', 'h7', 'Normal', 'ListParagraph', 'MyHeading2x', 'Heading0', 'Heading', 'Quote'];
const HLS = ['yellow', 'green', 'cyan', 'darkBlue', 'lightGray', 'black', 'white', 'foo', 'constructor', 'toString', '__proto__', 'valueOf', 'hasOwnProperty', 'Yellow', ''];
const FONTS = ['Aptos', "Fira 'Code'", 'Times New Roman', 'A&B'];
const COLORS = ['FF0000', '00ff00', '0000FF', 'auto', ''];
const BORDER_VALS = ['single', 'dashed', 'dotted', 'double', 'none', 'nil', 'wave', 'thick', 'dotDash'];

class Gen {
  constructor(seed) { this.rng = new Rng(seed); this.cmt = 0; this.note = 1; }
  below(n) { return this.rng.below(n); }
  pick(a) { return a[this.below(a.length)]; }
  chance(n) { return this.below(n) === 0; }
  int(lo, hi) { return lo + this.below(hi - lo + 1); }
  text(n = 4) { let s = ''; for (let i = this.int(1, n); i > 0; i--) s += (s && !this.chance(5) ? ' ' : '') + this.pick(WORDS); return s; }

  runProps() {
    const p = {};
    if (this.chance(4)) p.b = true;
    if (this.chance(4)) p.i = this.pick([true, '0', '1', 'false']);
    if (this.chance(6)) p.u = this.pick([true, 'double', 'none']);
    if (this.chance(8)) p.strike = true;
    if (this.chance(10)) p.dstrike = true;
    if (this.chance(8)) p.va = this.pick(['superscript', 'subscript', 'baseline']);
    if (this.chance(5)) p.sz = this.pick([20, 21, 24, 36, '0', '-2', 'x', '1e2']);
    if (this.chance(5)) p.color = this.pick(COLORS);
    if (this.chance(6)) p.hl = this.pick(HLS);
    if (this.chance(8)) p.font = this.pick(FONTS);
    if (this.chance(10)) p.caps = true;
    if (this.chance(12)) p.smallCaps = true;
    if (this.chance(10)) p.spacing = this.pick([20, -5, 0, 'x', 7.5]);
    return p;
  }

  run(opts = {}) {
    const r = { t: this.text(), ...this.runProps() };
    if (this.chance(9)) r.link = this.pick(['rIdLink', 'rIdLink2']);
    if (this.chance(12) && opts.cmts !== false) r.cmt = this.pick([[0], [1], [0, 1], [2], [7], [0, 0]]);
    if (this.chance(40) && opts.rev !== false) r.ins = true;
    if (this.chance(40) && opts.rev !== false) r.del = true;
    if (this.chance(25)) { r.t = this.pick([' ', '  ', ' lead', 'trail ', ' both ', '  x ', '']); }
    return r;
  }

  paraProps() {
    const p = {};
    if (this.chance(3)) p.style = this.pick(STYLES);
    if (this.chance(6)) p.jc = this.pick(['center', 'right', 'both', 'left', 'foo', 'distribute']);
    if (this.chance(6)) p.ind = { left: this.pick([0, 720, '12.5', 'x', -100, 1440]), ...(this.chance(2) ? { right: this.pick([360, 0, 'y']) } : {}), ...(this.chance(2) ? { firstLine: this.pick([360, -360, 0]) } : {}) };
    if (this.chance(10)) p.shd = this.pick(['FFFF00', 'auto', '', 'ABCDEF']);
    if (this.chance(10)) {
      p.borders = {};
      for (const side of ['top', 'left', 'bottom', 'right']) if (this.chance(2)) p.borders[side] = { val: this.pick(BORDER_VALS), sz: this.pick([2, 4, 12, 0, 'x']), color: this.pick(['FF0000', 'auto', undefined]), space: 1 };
    }
    if (this.chance(25)) p.markDel = this.int(1, 9);
    if (this.chance(15)) p.tabs = [{ pos: this.pick([720, 1440]), val: this.pick(['left', 'center', 'right']) }];
    if (this.chance(12)) p.spacing = { before: this.pick([0, 120, 240]), after: this.pick([0, 160]), line: this.pick([240, 360]) };
    return p;
  }

  paragraph(opts = {}) {
    const runs = [];
    for (let i = this.int(0, 4); i > 0; i--) {
      const k = this.below(14);
      if (k === 0) runs.push({ img: { w: this.pick([914400, 1828800, 0, 635000]), h: this.pick([457200, 914400, 100000]), rid: this.pick(['rIdImg', 'rIdImg', 'rIdImg2']), ...(this.chance(5) ? { anchor: { x: this.pick([0, 10000]), y: this.pick([0, 5000]) } } : {}) } });
      else if (k === 1 && opts.notes !== false) runs.push({ note: this.int(1, 3), kind: this.pick(['footnote', 'footnote', 'endnote']) });
      else if (k === 2) runs.push({ field: this.pick(['PAGE', 'NUMPAGES', 'REF bm1', 'DATE \\@ "dd/MM/yyyy"']), t: this.pick(['1', '2', 'x']) });
      else if (k === 3) runs.push({ tab: true });
      else if (k === 4) runs.push({ br: true });
      else runs.push(this.run(opts));
    }
    return { type: 'p', p: this.paraProps(), runs };
  }

  listRun() {
    const n = this.int(1, 7);
    const numId = this.pick([1001, 1002]);
    const out = [];
    let lvl = this.pick([0, 0, 1, 2]);
    for (let i = 0; i < n; i++) {
      const b = this.paragraph({ notes: false });
      b.p = { ...b.p, style: undefined, num: { ilvl: this.chance(25) ? this.pick(['-1', 'x', '2.5', '1e1', '', '9999', ' 3 ']) : lvl, numId: this.chance(8) ? this.pick([0, 1002, 1001, 5]) : numId } };
      out.push(b);
      lvl = Math.max(0, Math.min(8, lvl + this.pick([-2, -1, 0, 0, 1, 1, 2])));
    }
    return out;
  }

  tableDesc(depth = 0) {
    const cols = this.int(1, 4);
    const rows = [];
    for (let r = this.int(1, 4); r > 0; r--) {
      const row = [];
      for (let c = 0; c < cols; c++) {
        const blocks = [];
        for (let k = this.int(1, 2); k > 0; k--) blocks.push(this.paragraph({ notes: false }));
        if (depth < 2 && this.chance(8)) blocks.push(this.tableDesc(depth + 1));
        row.push({ blocks, ...(this.chance(5) ? { span: this.pick([2, 3, '0', 'x', '1e0']) } : {}), ...(this.chance(5) ? { shd: this.pick(['FFFF00', 'auto']) } : {}), ...(this.chance(6) ? { w: this.pick([1000, 3000]) } : {}) });
      }
      if (this.chance(10)) row.pop();
      rows.push(row);
    }
    return { type: 'tbl', rows, grid: Array.from({ length: cols }, () => this.pick([1500, 2500, 4000])), ...(this.chance(4) ? { borders: true } : {}) };
  }

  blocks(n) {
    const out = [];
    while (out.length < n) {
      const k = this.below(12);
      if (k === 0) out.push(this.tableDesc());
      else if (k === 1) out.push(...this.listRun());
      else out.push(this.paragraph());
    }
    return out;
  }

  document(n) {
    const d = { blocks: this.blocks(n) };
    if (this.chance(2)) d.comments = [0, 1, 2, 2].map((id) => ({ id, author: this.pick(['Ann', 'Bob & Co', '<b>', '', undefined]), paras: this.chance(3) ? [this.text(), this.text()] : [this.text(5)] }));
    if (this.chance(2)) d.footnotes = [1, 2, 3].map((id) => ({ id, blocks: [{ type: 'p', runs: [this.run({ cmts: false }), this.run({ cmts: false })] }, ...(this.chance(5) ? [this.paragraph({ notes: false, cmts: false })] : [])] }));
    if (this.chance(3)) d.endnotes = [1, 2].map((id) => ({ id, blocks: [{ type: 'p', runs: [{ t: this.text() }, { t: '&<>"' }] }] }));
    return d;
  }
}

const hand = () => {
  const P = (t, p, extra = {}) => ({ type: 'p', p, runs: [{ t, ...extra }] });
  const out = [];
  out.push(['export-basic', { blocks: [P('Title', { style: 'Title' }), P('Heading one', { style: 'Heading1' }), P('Body with **stars** and _under_ and |pipes| and `ticks`'), P('Bold', {}, { b: true }), P('Both', {}, { b: true, i: true, strike: true, u: true, va: 'superscript' })] }]);
  out.push(['export-lists', { blocks: [
    { type: 'p', p: { num: { ilvl: 0, numId: 1001 } }, runs: [{ t: 'a' }] }, { type: 'p', p: { num: { ilvl: 1, numId: 1002 } }, runs: [{ t: 'b' }] }, { type: 'p', p: { num: { ilvl: 2, numId: 1001 } }, runs: [{ t: 'c' }] },
    { type: 'p', p: { num: { ilvl: 0, numId: 1002 } }, runs: [{ t: 'd' }] }, P('para'), { type: 'p', p: { num: { ilvl: 1, numId: 1001 } }, runs: [{ t: 'e' }] }, { type: 'p', p: { num: { ilvl: 0, numId: 1001 } }, runs: [{ t: 'f' }] }] }]);
  out.push(['export-list-shallower-first', { blocks: ['1', '0', '2', '1', '0'].map((l) => ({ type: 'p', p: { num: { ilvl: l, numId: 1001 } }, runs: [{ t: `l${l}` }] })) }]);
  out.push(['export-list-skip', { blocks: ['0', '2', '1', '3', '0', '1'].map((l) => ({ type: 'p', p: { num: { ilvl: l, numId: 1002 } }, runs: [{ t: `l${l}` }] })) }]);
  out.push(['export-list-bad-levels', { blocks: ['-1', 'x', '1e1', '2.5', '', ' 3 ', '0'].map((l) => ({ type: 'p', p: { num: { ilvl: l, numId: 1001 } }, runs: [{ t: `l${l}` }] })) }]);
  out.push(['export-list-negative', { blocks: [{ type: 'p', p: { num: { ilvl: '-1', numId: 1001 } }, runs: [{ t: 'neg' }] }] }]);
  out.push(['export-list-huge', { blocks: [{ type: 'p', p: { num: { ilvl: '70000', numId: 1001 } }, runs: [{ t: 'huge' }] }] }]);
  out.push(['export-highlights', { blocks: [{ type: 'p', runs: HLS.map((h) => ({ t: `[${h}]`, hl: h })) }] }]);
  out.push(['export-tracked', { blocks: [
    { type: 'p', runs: [{ t: 'kept ' }, { t: 'inserted ', ins: true }, { t: 'deleted', del: true }] },
    { type: 'p', p: { markDel: 1 }, runs: [{ t: 'merged-away ' }, { t: 'x', del: true }] }, { type: 'p', p: { jc: 'center' }, runs: [{ t: 'next paragraph' }] },
    { type: 'p', p: { markDel: 2 }, runs: [{ t: 'only mark' }] }, { type: 'p', p: { markDel: 3 }, runs: [] }, { type: 'p', runs: [{ t: 'tail' }] },
    { type: 'p', p: { markDel: 4 }, runs: [{ t: 'last para with deleted mark' }] }] }]);
  out.push(['export-tracked-table', { blocks: [P('before'), { type: 'tbl', rows: [[{ blocks: [{ type: 'p', p: { markDel: 1 }, runs: [{ t: 'c1 ' }] }, P('c2', {}, { del: true }), { type: 'tbl', rows: [[{ blocks: [{ type: 'p', p: { markDel: 2 }, runs: [{ t: 'nested ' }] }, P('nested2')] }]] }] }]] }, { type: 'p', p: { markDel: 5 }, runs: [{ t: 'x' }] }, P('after')] }]);
  out.push(['export-notes', { blocks: [{ type: 'p', runs: [{ t: 'a' }, { note: 1 }, { t: 'b' }, { note: 2, kind: 'endnote' }, { note: 1 }, { note: 9 }] }], footnotes: [{ id: 1, blocks: [{ type: 'p', runs: [{ t: ' a&b <c> ' }, { t: 'more', b: true }] }, { type: 'p', runs: [{ t: 'second para' }] }] }], endnotes: [{ id: 2, blocks: [{ type: 'p', runs: [{ t: 'end "note" & more' }] }] }] }]);
  out.push(['export-comments', { blocks: [{ type: 'p', runs: [{ t: 'one', cmt: 0 }, { t: 'two', cmt: [1, 0] }, { t: 'three', cmt: 5 }, { t: 'four', cmt: [0] }] }, { type: 'tbl', rows: [[{ blocks: [{ type: 'p', runs: [{ t: 'cell', cmt: 1 }] }] }]] }], comments: [{ id: 0, author: 'Ann', paras: ['first <b>', 'second'] }, { id: 1, paras: ['no author'] }, { id: 0, author: 'Dup', paras: ['dup id'] }] }]);
  out.push(['export-table', { blocks: [{ type: 'tbl', rows: [[{ blocks: [P('h1')] }, { blocks: [P('h2 | pipe')], shd: 'FFFF00' }, { blocks: [P('h3')] }], [{ blocks: [P('a'), P('b')], span: 2 }, { blocks: [{ type: 'tbl', rows: [[{ blocks: [P('nested')] }]] }] }], []] }, P('after'), { type: 'tbl', rows: [] }] }]);
  out.push(['export-images', { blocks: [{ type: 'p', runs: [{ t: 'x' }, { img: { w: 914400, h: 457200 } }, { img: { w: 1000, h: 2000, rid: 'rIdImg2' } }, { img: { w: 914400, h: 457200, anchor: { x: 100, y: 200 } } }] }], images: ['\u0089PNG-bytes-1', 'GIF89a-two'] }]);
  out.push(['export-para-styles', { blocks: [{ type: 'p', p: { jc: 'center', ind: { left: 720, right: 360, firstLine: -360 }, shd: 'FFFF00', borders: { top: { val: 'single', sz: 12, color: 'FF0000' }, left: { val: 'dashed' }, bottom: { val: 'nil' }, right: { val: 'wave', sz: 0 } } }, runs: [{ t: 'styled', color: '00FF00', sz: 24, font: "O'Neil", caps: true, smallCaps: true, spacing: 20, hl: 'yellow', link: 'rIdLink', cmt: 0 }] }], comments: [{ id: 0, author: 'A', paras: ['c'] }] }]);
  out.push(['export-empty', { blocks: [] }]);
  out.push(['export-empty-paras', { blocks: [{ type: 'p', runs: [] }, { type: 'p', runs: [{ t: '' }] }, { type: 'p', runs: [{ t: '   ' }] }, P('x')] }]);
  out.push(['export-whitespace-emphasis', { blocks: [{ type: 'p', runs: [{ t: '  bold lead', b: true }, { t: 'trail  ', i: true }, { t: '   ', b: true }, { t: ' nb ', b: true, i: true }, { t: ' x ', strike: true, u: true, va: 'subscript' }, { t: '﻿bom', b: true }] }] }]);
  out.push(['export-fields', { blocks: [{ type: 'p', runs: [{ t: 'Page ' }, { field: 'PAGE', t: '3', b: true }, { t: ' of ' }, { field: 'NUMPAGES', t: '9' }] }] }]);
  return out;
};

const seeded = (seed, count, size) => {
  const g = new Gen(seed);
  return Array.from({ length: count }, (_, i) => [`export-seed-${i}`, g.document(1 + g.below(size))]);
};

/** `[{ key, parts }]` for the export facets. */
export function quirkExport(seed = QUIRK_D5A_SEED, count = QUIRK_EXPORT_SEEDED) {
  return [...hand(), ...seeded(seed, count, 14)].map(([name, d]) => ({ key: `quirk/d5a/${name}.docx`, parts: render(d) }));
}

// ───────────────────────── pairs (compare) ─────────────────────────

const clone = (x) => JSON.parse(JSON.stringify(x));

/** Edits one document descriptor into another: the kinds a diff must tell apart. */
function mutate(g, d) {
  const e = clone(d);
  const kinds = [];
  const n = g.int(1, 4);
  for (let i = 0; i < n; i++) {
    const k = g.below(16);
    const bs = e.blocks;
    const at = () => g.below(Math.max(1, bs.length));
    const paras = () => bs.map((b, i) => [b, i]).filter(([b]) => b.type === 'p' && b.runs.length);
    const tables = () => bs.map((b, i) => [b, i]).filter(([b]) => b.type === 'tbl' && b.rows.length);
    if (k === 0) { bs.splice(at(), 0, g.paragraph({ notes: false })); kinds.push('insert-p'); }
    else if (k === 1 && bs.length) { bs.splice(at(), 1); kinds.push('delete'); }
    else if (k === 2) { const ps = paras(); if (ps.length) { const [b] = g.pick(ps); b.runs[g.below(b.runs.length)].t = g.text(); kinds.push('change-text'); } }
    else if (k === 3) { const ps = paras(); if (ps.length) { const [b] = g.pick(ps); const r = b.runs.find((r) => r.t !== undefined); if (r) { r.t += ` ${g.pick(WORDS)}`; kinds.push('append-word'); } } }
    else if (k === 4) { const ps = paras(); if (ps.length) { const [b] = g.pick(ps); const r = b.runs.find((r) => typeof r.t === 'string' && r.t.length > 1 && !r.field); if (r) { const cut = g.int(1, r.t.length - 1); const { t, ...rest } = r; b.runs.splice(b.runs.indexOf(r), 1, { ...rest, t: t.slice(0, cut) }, { ...rest, t: t.slice(cut) }); kinds.push('split-run'); } } }
    else if (k === 5) { const ps = paras(); if (ps.length) { const [b] = g.pick(ps); b.runs[0].b = !b.runs[0].b ? true : undefined; kinds.push('toggle-bold'); } }
    else if (k === 6) { const ps = paras(); if (ps.length) { const [b] = g.pick(ps); b.p = { ...(b.p ?? {}), jc: g.pick(['center', 'right', 'left', undefined]) }; kinds.push('align'); } }
    else if (k === 7) { const ps = paras(); if (ps.length) { const [b] = g.pick(ps); b.runs[0].sz = g.pick([20, 22, 28]); kinds.push('size'); } }
    else if (k === 8) { const ts = tables(); if (ts.length) { const [t] = g.pick(ts); t.rows.splice(g.below(t.rows.length + 1), 0, t.rows[g.below(t.rows.length)].map((c) => clone(c))); kinds.push('table-insert-row'); } }
    else if (k === 9) { const ts = tables(); if (ts.length) { const [t] = g.pick(ts); t.rows.splice(g.below(t.rows.length), 1); kinds.push('table-delete-row'); } }
    else if (k === 10) { const ts = tables(); if (ts.length) { const [t] = g.pick(ts); const row = g.pick(t.rows); const c = row.length ? g.pick(row) : undefined; const cp = c?.blocks.find((b) => b.type === 'p'); if (cp) { cp.runs = [{ t: g.text() }]; kinds.push('table-cell-text'); } } }
    else if (k === 11) { const ts = tables(); if (ts.length) { const [t] = g.pick(ts); const row = g.pick(t.rows); if (row.length) { g.pick(row).shd = g.pick(['00FF00', 'FFFF00']); kinds.push('table-cell-shade'); } } }
    else if (k === 12) { const ps = paras().filter(([b]) => b.runs.some((r) => r.img)); if (ps.length) { const [b] = g.pick(ps); const r = b.runs.find((r) => r.img); r.img = { ...r.img, w: r.img.w + 91440 }; kinds.push('image-resize'); } }
    else if (k === 13) { if (bs.length > 1) { const [b] = bs.splice(at(), 1); bs.splice(at(), 0, b); kinds.push('move'); } }
    else if (k === 14) { const ps = paras(); if (ps.length) { const [b] = g.pick(ps); b.runs[0].ins = true; kinds.push('tracked-insert'); } }
    else if (k === 15) { const ps = paras(); if (ps.length) { const [b] = g.pick(ps); b.p = { ...(b.p ?? {}), markDel: 7 }; kinds.push('mark-del'); } }
  }
  if (g.chance(10)) { e.images = ['IMG-CHANGED', 'IMG-TWO']; kinds.push('image-bytes'); }
  return [e, kinds];
}

/** `[{ key, a, b }]` (parts of both packages): the same document, one edit, several edits, unrelated. */
export function quirkPairs(seed = QUIRK_D5A_SEED + 1, count = QUIRK_PAIRS_SEEDED) {
  const g = new Gen(seed);
  const out = [];
  const add = (name, da, db) => out.push({ key: `quirk/d5a-pair/${name}`, a: render(da), b: render(db) });
  const small = [];
  for (let i = 0; i < count; i++) {
    const d = g.document(1 + g.below(24));
    const [e, kinds] = mutate(g, d);
    small.push(d);
    add(`seed-${i}-${kinds.join('+') || 'none'}`, d, e);
  }
  // identical and unrelated pairs, repeated paragraphs (the histogram anchors), long runs of equals
  for (let i = 0; i < 12; i++) add(`unrelated-${i}`, small[i], small[(i + 7) % small.length]);
  const P = (t) => ({ type: 'p', runs: [{ t }] });
  const rep = (n, t) => Array.from({ length: n }, () => P(t));
  add('dups-insert', { blocks: [...rep(5, 'same'), P('x'), ...rep(5, 'same')] }, { blocks: [...rep(5, 'same'), P('x'), P('y'), ...rep(5, 'same')] });
  add('dups-shift', { blocks: [...rep(6, 'same'), P('end')] }, { blocks: [P('start'), ...rep(6, 'same'), P('end')] });
  add('all-different', { blocks: Array.from({ length: 40 }, (_, i) => P(`a${i}`)) }, { blocks: Array.from({ length: 40 }, (_, i) => P(`b${i}`)) });
  add('reverse', { blocks: Array.from({ length: 30 }, (_, i) => P(`p${i}`)) }, { blocks: Array.from({ length: 30 }, (_, i) => P(`p${29 - i}`)) });
  add('empty-vs-doc', { blocks: [] }, { blocks: [P('a'), P('b')] });
  add('doc-vs-empty', { blocks: [P('a'), P('b')] }, { blocks: [] });
  add('empty-vs-empty', { blocks: [] }, { blocks: [] });
  add('words-1', { blocks: [P('the quick brown fox jumps over the lazy dog')] }, { blocks: [P('the quick red fox leaps over the lazy cat')] });
  add('words-none-common', { blocks: [P('alpha beta')] }, { blocks: [P('gamma delta')] });
  add('words-space-only-common', { blocks: [P('a b c')] }, { blocks: [P('x y z')] });
  add('words-unicode', { blocks: [P('日本語 😀 emoji x')] }, { blocks: [P('日本語 😀 emoji y z')] });
  add('format-only', { blocks: [{ type: 'p', runs: [{ t: 'same text' }] }] }, { blocks: [{ type: 'p', runs: [{ t: 'same text', b: true }] }] });
  add('format-runs-split', { blocks: [{ type: 'p', runs: [{ t: 'same text', b: true }] }] }, { blocks: [{ type: 'p', runs: [{ t: 'same ', b: true }, { t: 'text', b: true }] }] });
  add('image-resize', { blocks: [{ type: 'p', runs: [{ img: { w: 914400, h: 457200 } }] }] }, { blocks: [{ type: 'p', runs: [{ img: { w: 1828800, h: 457200 } }] }] });
  add('image-bytes', { blocks: [{ type: 'p', runs: [{ img: { w: 914400, h: 457200 } }] }], images: ['AAAA'] }, { blocks: [{ type: 'p', runs: [{ img: { w: 914400, h: 457200 } }] }], images: ['AAAB'] });
  add('table-rows', { blocks: [{ type: 'tbl', rows: [[{ blocks: [P('a')] }], [{ blocks: [P('b')] }], [{ blocks: [P('c')] }]] }] }, { blocks: [{ type: 'tbl', rows: [[{ blocks: [P('a')] }], [{ blocks: [P('x')] }], [{ blocks: [P('c')] }], [{ blocks: [P('d')] }]] }] });
  add('table-vs-para', { blocks: [{ type: 'tbl', rows: [[{ blocks: [P('a')] }]] }] }, { blocks: [P('a')] });
  add('tracked-final', { blocks: [{ type: 'p', runs: [{ t: 'kept ' }, { t: 'gone', del: true }] }, { type: 'p', p: { markDel: 1 }, runs: [{ t: 'x' }] }, P('y')] }, { blocks: [P('kept '), P('xy')] });
  add('fields', { blocks: [{ type: 'p', runs: [{ field: 'PAGE', t: '1' }] }] }, { blocks: [{ type: 'p', runs: [{ field: 'PAGE', t: '2' }] }] });
  add('fields-other', { blocks: [{ type: 'p', runs: [{ field: 'PAGE', t: '1' }] }] }, { blocks: [{ type: 'p', runs: [{ field: 'NUMPAGES', t: '1' }] }] });
  return out;
}

/** The inputs file (one part per line, TAB separated; `\`, LF, CR and TAB escaped), one `@input` per package. */
export function inputsText(inputs, title) {
  const escp = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
  const lines = [`# uo-docx D5a synthetic packages (js/quirk-d5a.mjs, ${title}): \`@input <key>\`, then \`<path>\\t<escaped text>\` per part`];
  for (const { key, parts } of inputs) {
    lines.push(`@input ${key}`);
    for (const [path, data] of parts) lines.push(`${path}\t${escp(data)}`);
  }
  return lines.join('\n') + '\n';
}
