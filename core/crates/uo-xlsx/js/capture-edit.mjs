// Edit-operation captures of the Sheet element (frontier C5a; record core/docs/modules/uo-xlsx.md "C5a — as built").
// The frozen TS `<universal-sheet>` runs headless on the fake DOM (harness/build-harness.mjs, same host injection as
// capture-sheet.mjs: TZ=UTC, `Date` frozen at 2025-01-15 12:00 UTC, seeded `Math.random`). Every SCRIPT opens a workbook
// FRESH in a new element, runs a short sequence of steps (one operation each, ending with undo/redo checks) and records
// facets after every step; scripts are independent (one unsupported step cannot lose the rest). Steps use literal
// arguments (no random choice): the coverage is the scenario table below.
//
//   node core/crates/uo-xlsx/js/capture-edit.mjs                 # -> tests/captures/sheet-edit.{capture,side.gz}
//   options: --only <substring of the script key>  --out <file>  --ts-root <dir>  --no-xml (no part XML in the sidecar)
//            (the per-operation counts and any throwing step are printed after the run)
//
// Script key `edit/<scenario>/<workbook>/<lang>`; workbook = `x1/<NNNN>` (tests/captures/workbooks/x1) or `samples/<file>`;
// lang = the `formula-locale` attribute of the element (pt-BR | en-US), which also picks the entry texts.
// Facets of a record (`len:sha256[0..32]` digests of canonical texts, the texts are in the gz sidecar as lines
// `<key>\t<facet>\t<text>`; a facet whose text equals the previous step's is not repeated in the sidecar):
//   input        len:sha16 of the file;  scenario/wb/lang/ops  what the script is
//   o.*          the state after the OPEN (step o) and e<k>.* after step k (k = 0..n-1), the same facets:
//   e<k>         the step: `<op> | <description>`;  e<k>.r  the call's result (JSON, Sets as arrays) or `throw:<msg>`
//   .sel         active sheet | active cell col,row | range c1,r1,c2,r2   (public `getSelection` + `activeSheet`)
//   .undo / .redo   the stacks: `<n>:` + `label[sheetIdx:cells:S?>cells:S?]` per entry, oldest first (S = StructSnap)
//   .utop        digest of the TOP undo entry (before/after: cells {col,row,edit,style,exists}, struct canonical JSON);
//                the sidecar keeps its canonical JSON when it changed
//   .dirty       `dirty=<isDirty> dirtySheets=<i,..> merge=<i,..> view=<i,..>` (the private sets the save reads)
//   .sheets      sheet names | zoom | frozen panes (active sheet) | gridlines | hasClipboard | canUndo/canRedo
//   .st<i>       STORE facet of sheet i (as `sheet-x1`: one canonical JSON line per cell {c,r,t,v,s,f,fa,cm,vm,is,ref}
//                sorted by (row,col)); sidecar: the lines that CHANGED vs the previous step (`+line` added or changed,
//                `-r,c` removed), the first step of a sheet the whole sheet
//   .ws<i>       readWorksheet-style model of the live worksheet WITHOUT rows (canonical JSON: cols, merges, pane,
//                showGridLines, …); sidecar: full text when changed
//   .rw<i>       row metadata lines (height/customHeight/hidden/style/customFormat)
//   .xf          the number of cellXfs + numFmts after the step (the XfPool growth) | .d<i> display text of the active sheet
//   .save        the save after the step: `<parts>:<digest of name/len/sha lines>`; .chg parts that differ from the INPUT;
//                .chgp parts that differ from the previous step's save; sidecar `.save:<part>` = the XML of the .chgp
//                parts (as JSON strings; sheet parts over 40 KB, drawings and images are digests only)
// The per-operation counts are in the capture metadata (`@ops <op>=<steps>/<workbooks>/<langs>`).
process.env.TZ = 'UTC';
const { readFileSync, mkdirSync, createWriteStream } = await import('node:fs');
const { join, dirname } = await import('node:path');
const { createHash } = await import('node:crypto');
const { createGzip } = await import('node:zlib');
const { CaptureWriter, Rng } = await import('../../uo-testkit/js/testkit.mjs');
const { canonJson } = await import('../../uo-pptx/js/lib/canon-json.mjs');
const C = await import('./lib/common.mjs');
const { unzip } = await import('./lib/unzip.mjs');
const { loadHarness } = await import('./harness/build-harness.mjs');

const a = C.args();
const root = C.tsRoot(a);
const only = a.opt('--only');
const XML = !a.flag('--no-xml');

// ---------------------------------------------------------------- host injection (C1)
const FIXED = Date.UTC(2025, 0, 15, 12, 0, 0);
const RealDate = Date;
class FixedDate extends RealDate {
  constructor(...x) { if (x.length === 0) super(FIXED); else super(...x); }
  static now() { return FIXED; }
}
globalThis.Date = FixedDate;
let rng = new Rng(1);
Math.random = () => rng.next() / 4294967296;
// console noise of the element (recalc warnings) is not part of the capture
const quiet = (f) => { const w = console.warn, e = console.error; console.warn = console.error = () => {}; try { return f(); } finally { console.warn = w; console.error = e; } };

const h = await loadHarness(root);
const { sheetCore: SC } = h;

// ---------------------------------------------------------------- helpers
const sha = (s) => createHash('sha256').update(s).digest('hex');
const dig = (s) => { const b = Buffer.from(s, 'utf8'); return `${b.length}:${sha(b).slice(0, 32)}`; };
const listDig = (lines) => `${lines.length}:${sha(Buffer.from(lines.join('\n'), 'utf8')).slice(0, 32)}`;
const jsonSets = (kk, vv) => (vv instanceof Set ? [...vv].sort((x, y) => x - y) : vv);

/** The store facet lines of a live worksheet (as capture-sheet.mjs `storeOf`) + the row metadata lines. */
function storeOf(ws) {
  const map = new Map();
  const rowMeta = [];
  for (const row of ws.rows) {
    const meta = { customFormat: row.customFormat, customHeight: row.customHeight, hidden: row.hidden, ht: row.height, r: row.r, s: row.style, spans: row.spans, thickBot: row.thickBot, thickTop: row.thickTop };
    if (Object.keys(meta).some((k) => k !== 'r' && meta[k] !== undefined)) rowMeta.push(canonJson(meta));
    for (const c of row.cells) {
      let col;
      try { col = SC.refToRowCol(c.ref).col; } catch { col = -1; }
      const canonRef = col > 0 ? SC.rowColToRef(col, row.r) : undefined;
      map.set(row.r * 20000 + col, { c: col, cm: c.cm, f: c.formula, fa: c.formulaAttrs, is: c.inlineString, r: row.r, ref: c.ref !== canonRef ? c.ref : undefined, s: c.style, t: c.type, v: c.value, vm: c.vm });
    }
  }
  const keys = [...map.keys()].sort((x, y) => x - y);
  return { lines: new Map(keys.map((k) => [`${map.get(k).r},${map.get(k).c}`, canonJson(map.get(k))])), rowMeta };
}

/** Sidecar delta of a line map vs the previous one: `+line` (new or changed), `-r,c` (gone). */
function delta(prev, cur) {
  const out = [];
  for (const [k, l] of cur) if (prev?.get(k) !== l) out.push(`+${l}`);
  if (prev) for (const k of prev.keys()) if (!cur.has(k)) out.push(`-${k}`);
  return out;
}

function partsOf(bytes) {
  const parts = unzip(bytes);
  return new Map(parts.map((p) => [p.name, p.data]));
}

// ---------------------------------------------------------------- texts per language
const T = {
  'pt-BR': { num: '1,5', neg: '-0,5', pct: '50%', date: '12/03/2024', money: 'R$ 10,00', big: '1.234,5', text: 'texto ção', quoted: "'007", bool: 'VERDADEIRO', sum: '=SOMA(A1:A5)*2', iff: '=SE(A2>1;"s";"n")', cat: '=A1&"x"', seq: '=SEQUÊNCIA(3;2)', bad: '=SOMA(', today: '=HOJE()', ref: '=B2+1', rnd: '=ALEATÓRIO()', find: 'ção', findNum: '1', sumName: 'SOMA' },
  'en-US': { num: '1.5', neg: '-0.5', pct: '50%', date: '3/12/2024', money: '$10.00', big: '1,234.5', text: 'texto ção', quoted: "'007", bool: 'TRUE', sum: '=SUM(A1:A5)*2', iff: '=IF(A2>1,"s","n")', cat: '=A1&"x"', seq: '=SEQUENCE(3,2)', bad: '=SUM(', today: '=TODAY()', ref: '=B2+1', rnd: '=RAND()', find: 'ção', findNum: '1', sumName: 'SUM' },
};
const key = (k) => ({ key: k, preventDefault() {}, stopPropagation() {}, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, code: k });

// ---------------------------------------------------------------- scenarios
// step = [op, description, (el, ctx) => result]; `ctx.t` the texts of the language, `ctx.host` the host-library hooks.
// `hint` picks workbooks that have the feature the scenario needs (merge | frozen | hidden | tables | names | none).
const S = (id, hint, ops, steps) => ({ id, hint, ops, steps });
const sel = (r) => (el) => { el.selectRange(r); };
const scenarios = [
  S('entry-const', 'none', ['entry.text', 'entry.clear', 'undo', 'redo'], ({ t }) => [
    ['entry.text', 'A1 number', (el) => { el.selectRange('A1'); return el.setActiveCellText(t.num); }],
    ['entry.text', 'B2 text with accents', (el) => { el.selectRange('B2'); return el.setActiveCellText(t.text); }],
    ['entry.text', 'C3 percent', (el) => { el.selectRange('C3'); return el.setActiveCellText(t.pct); }],
    ['entry.text', 'D4 date', (el) => { el.selectRange('D4'); return el.setActiveCellText(t.date); }],
    ['entry.text', 'E5 currency', (el) => { el.selectRange('E5'); return el.setActiveCellText(t.money); }],
    ['entry.text', 'F6 quoted text', (el) => { el.selectRange('F6'); return el.setActiveCellText(t.quoted); }],
    ['entry.text', 'G7 boolean', (el) => { el.selectRange('G7'); return el.setActiveCellText(t.bool); }],
    ['entry.text', 'B30 far below the used range (grows)', (el) => { el.selectRange('B30'); return el.setActiveCellText(t.big); }],
    ['entry.clear', 'A1 cleared by empty text', (el) => { el.selectRange('A1'); return el.setActiveCellText(''); }],
    ['undo', 'undo clear', (el) => el.undo()],
    ['undo', 'undo far entry', (el) => el.undo()],
    ['redo', 'redo far entry', (el) => el.redo()],
  ]),
  S('entry-formula', 'none', ['entry.formula', 'entry.refused', 'calc.recalc', 'undo', 'redo'], ({ t }) => [
    ['entry.formula', 'D1 sum', (el) => { el.selectRange('D1'); return el.setActiveCellText(t.sum); }],
    ['entry.formula', 'D2 concat', (el) => { el.selectRange('D2'); return el.setActiveCellText(t.cat); }],
    ['entry.refused', 'D3 unbalanced formula refused', (el) => { el.selectRange('D3'); return el.setActiveCellText(t.bad); }],
    ['entry.formula', 'E1 spill', (el) => { el.selectRange('E1'); return el.setActiveCellText(t.seq); }],
    ['entry.formula', 'D4 date function (format hint)', (el) => { el.selectRange('D4'); return el.setActiveCellText(t.today); }],
    ['entry.formula', 'D5 if', (el) => { el.selectRange('D5'); return el.setActiveCellText(t.iff); }],
    ['entry.text', 'E1 overwrite the spill anchor with a constant', (el) => { el.selectRange('E1'); return el.setActiveCellText(t.num); }],
    ['calc.recalc', 'recalculate', (el) => el.recalculate()],
    ['undo', 'undo constant over spill', (el) => el.undo()],
    ['undo', 'undo date function', (el) => el.undo()],
    ['redo', 'redo date function', (el) => el.redo()],
  ]),
  S('entry-editor', 'none', ['entry.editor', 'key.delete', 'entry.set', 'undo', 'redo'], ({ t }) => [
    ['entry.editor', 'beginEdit A2, editor value, commit', (el) => { el.beginEdit(1, 2, t.ref, false); el.editor.value = t.ref; return el.commitEdit(); }],
    ['entry.editor', 'beginEdit B3 then cancel', (el) => { el.beginEdit(2, 3, 'abc', false); el.cancelEdit(); return 'cancelled'; }],
    ['entry.editor', 'typing a character (keydown) then commit', (el) => { el.selectRange('C2'); el.onKeyDown(key('x')); el.editor.value = t.num; return el.commitEdit(); }],
    ['entry.editor', 'F2 on a formula cell, commit unchanged', (el) => { el.selectRange('A2'); el.onKeyDown(key('F2')); return el.commitEdit(); }],
    ['entry.editor', 'refused entry keeps editing (false)', (el) => { el.beginEdit(3, 3, t.bad, false); const r = el.commitEdit(); el.cancelEdit(); return r; }],
    ['key.delete', 'Delete on one cell', (el) => { el.selectRange('C2'); el.onKeyDown(key('Delete')); }],
    ['key.delete', 'Backspace on a range (clear contents)', (el) => { el.selectRange('A1:B2'); el.onKeyDown(key('Backspace')); }],
    ['entry.set', 'setCellValue (no undo record)', (el) => el.setCellValue(4, 4, t.neg)],
    ['undo', 'undo', (el) => el.undo()],
    ['undo', 'undo', (el) => el.undo()],
    ['redo', 'redo', (el) => el.redo()],
  ]),
  S('fmt-font', 'none', ['fmt.font', 'fmt.fill', 'fmt.border', 'fmt.format', 'undo', 'redo'], () => [
    ['fmt.font', 'B2:C4 bold', (el) => { el.selectRange('B2:C4'); el.setFont({ bold: true }); }],
    ['fmt.font', 'B2:C4 size 14, colour, italic', (el) => { el.setFont({ size: 14, color: '#C00000', italic: true }); }],
    ['fmt.font', 'A1 font name, underline, strike', (el) => { el.selectRange('A1'); el.setFont({ name: 'Arial', underline: true, strike: true }); }],
    ['fmt.fill', 'D2:E3 fill', (el) => { el.selectRange('D2:E3'); el.setFormat({ fill: '#FFFF00' }); }],
    ['fmt.fill', 'D2 fill removed', (el) => { el.selectRange('D2'); el.setFormat({ fill: null }); }],
    ['fmt.border', 'B2:C4 all', (el) => { el.selectRange('B2:C4'); el.setBorders('all'); }],
    ['fmt.border', 'B2:C4 thick outside', (el) => { el.setBorders('thickOutside', '#0000FF'); }],
    ['fmt.border', 'B2:C4 bottom', (el) => { el.setBorders('bottom'); }],
    ['fmt.border', 'B2:C4 none', (el) => { el.setBorders('none'); }],
    ['fmt.format', 'F1:F2 font+fill+border+align in one action', (el) => { el.selectRange('F1:F2'); el.setFormat({ font: { bold: true }, fill: '#C6EFCE', align: { horizontal: 'center' }, border: { bottom: { style: 'thin', color: '#000000' } } }); }],
    ['undo', 'undo', (el) => el.undo()],
    ['undo', 'undo', (el) => el.undo()],
    ['redo', 'redo', (el) => el.redo()],
  ]),
  S('fmt-number-align', 'none', ['fmt.numfmt', 'fmt.decimals', 'fmt.align', 'undo', 'redo'], () => [
    ['fmt.numfmt', 'B2:B4 percent', (el) => { el.selectRange('B2:B4'); el.setNumberFormat('percent'); }],
    ['fmt.numfmt', 'B2:B4 currency', (el) => { el.setNumberFormat('currency'); }],
    ['fmt.numfmt', 'B2:B4 short date', (el) => { el.setNumberFormat('shortDate'); }],
    ['fmt.numfmt', 'B2:B4 custom code', (el) => { el.setNumberFormat({ code: '#,##0.000' }); }],
    ['fmt.decimals', 'B2:B4 +1 decimal', (el) => { el.adjustDecimals(1); }],
    ['fmt.decimals', 'B2:B4 -1 decimal', (el) => { el.adjustDecimals(-1); }],
    ['fmt.align', 'C2:C3 centre + wrap', (el) => { el.selectRange('C2:C3'); el.setAlignment({ horizontal: 'center', wrapText: true }); }],
    ['fmt.align', 'C2:C3 indent 2 left, vertical top', (el) => { el.setAlignment({ horizontal: 'left', indent: 2, vertical: 'top' }); }],
    ['fmt.align', 'C2:C3 alignment reset', (el) => { el.setAlignment({ horizontal: null, vertical: null }); }],
    ['undo', 'undo', (el) => el.undo()],
    ['undo', 'undo', (el) => el.undo()],
    ['redo', 'redo', (el) => el.redo()],
  ]),
  S('fmt-lines', 'none', ['fmt.lines', 'clear.formats', 'undo', 'redo'], () => [
    ['fmt.lines', 'column B fill (whole column)', (el) => { el.selectRange('B:B'); el.setFormat({ fill: '#DDEBF7' }); }],
    ['fmt.lines', 'rows 2:3 bold (whole rows)', (el) => { el.selectRange('2:3'); el.setFont({ bold: true }); }],
    ['fmt.lines', 'select all, number format', (el) => { el.selectAll(); el.setNumberFormat({ code: '0.00' }); }],
    ['fmt.lines', 'columns C:D borders', (el) => { el.selectRange('C:D'); el.setBorders('all'); }],
    ['clear.formats', 'column B clear formats (whole)', (el) => { el.selectRange('B:B'); el.clear('formats'); }],
    ['undo', 'undo', (el) => el.undo()],
    ['undo', 'undo', (el) => el.undo()],
    ['redo', 'redo', (el) => el.redo()],
  ]),
  S('merge', 'merge', ['merge', 'unmerge', 'undo', 'redo'], () => [
    ['merge', 'B2:C3 merge and centre', (el) => { el.selectRange('B2:C3'); return [el.mergeLosesData(), el.merge('center')]; }],
    ['merge', 'D2:F4 merge across', (el) => { el.selectRange('D2:F4'); el.merge('across'); }],
    ['merge', 'A6:B7 merge cells', (el) => { el.selectRange('A6:B7'); el.merge('cells'); }],
    ['merge', 'B2:D3 overlapping an existing merge', (el) => { el.selectRange('B2:D3'); el.merge('cells'); }],
    ['unmerge', 'unmerge A6', (el) => { el.selectRange('A6'); el.unmerge(); }],
    ['unmerge', 'unmerge a selection with no merge (no-op)', (el) => { el.selectRange('H20'); el.unmerge(); }],
    ['undo', 'undo', (el) => el.undo()],
    ['undo', 'undo', (el) => el.undo()],
    ['redo', 'redo', (el) => el.redo()],
  ]),
  S('fill-clipboard', 'none', ['fill.down', 'fill.right', 'clip.copy', 'clip.cut', 'clip.pasteValues', 'clip.pasteText', 'undo', 'redo'], ({ t }) => [
    ['entry.text', 'seed A1', (el) => { el.selectRange('A1'); return el.setActiveCellText(t.sum); }],
    ['fill.down', 'A1:A5 down (formula shifts)', (el) => { el.selectRange('A1:A5'); el.fill('down'); }],
    ['fill.right', 'A1:D2 right', (el) => { el.selectRange('A1:D2'); el.fill('right'); }],
    ['fill.down', 'A3 alone (copies the row above)', (el) => { el.selectRange('A3'); el.fill('down'); }],
    ['clip.copy', 'copy A1:B2 then paste at D6', (el) => { el.selectRange('A1:B2'); const txt = el.copy(); el.selectRange('D6'); el.paste(); return txt; }],
    ['clip.cut', 'cut A1:A2 then paste at F8', (el) => { el.selectRange('A1:A2'); const txt = el.cut(); el.selectRange('F8'); el.paste(); return txt; }],
    ['clip.pasteValues', 'copy D6:E7 then paste values at H2', (el) => { el.selectRange('D6:E7'); el.copy(); el.selectRange('H2'); el.paste(undefined, { valuesOnly: true }); }],
    ['clip.pasteText', 'external TSV text pasted at A9', (el) => { el.selectRange('A9'); el.paste(`1\t${t.text}\r\n${t.date}\t${t.sum}\r\n`); }],
    ['undo', 'undo', (el) => el.undo()],
    ['undo', 'undo', (el) => el.undo()],
    ['redo', 'redo', (el) => el.redo()],
  ]),
  S('clear', 'none', ['clear.contents', 'clear.formats', 'clear.all', 'undo', 'redo'], () => [
    ['fmt.font', 'B2:C3 bold (so formats exist)', (el) => { el.selectRange('B2:C3'); el.setFont({ bold: true }); }],
    ['clear.contents', 'A1:C3 contents', (el) => { el.selectRange('A1:C3'); el.clear('contents'); }],
    ['clear.formats', 'A1:C3 formats', (el) => { el.clear('formats'); }],
    ['clear.all', 'D1:F4 all', (el) => { el.selectRange('D1:F4'); el.clear('all'); }],
    ['clear.all', 'rows 1:2 all (whole rows)', (el) => { el.selectRange('1:2'); el.clear('all'); }],
    ['clear.contents', 'column A contents (whole column)', (el) => { el.selectRange('A:A'); el.clear('contents'); }],
    ['undo', 'undo', (el) => el.undo()],
    ['undo', 'undo', (el) => el.undo()],
    ['redo', 'redo', (el) => el.redo()],
  ]),
  S('find-replace', 'none', ['find', 'replace', 'replaceAll', 'undo', 'redo'], ({ t }) => [
    ['entry.text', 'seed B2', (el) => { el.selectRange('B2'); return el.setActiveCellText(t.text); }],
    ['entry.text', 'seed C3', (el) => { el.selectRange('C3'); return el.setActiveCellText(t.text); }],
    ['find', 'find the accent word from A1', (el) => { el.selectRange('A1'); return el.find(t.find); }],
    ['find', 'find backwards, values', (el) => el.find(t.find, { backwards: true, lookIn: 'values' })],
    ['find', 'find all sheets, whole cell, match case', (el) => el.find(t.text, { allSheets: true, wholeCell: true, matchCase: true })],
    ['replace', 'replace on the active cell', (el) => { el.selectRange('B2'); return el.replace('texto', 'TXT'); }],
    ['replaceAll', 'replace all "ção"', (el) => el.replaceAll(t.find, 'cao')],
    ['replaceAll', 'replace all digit 1 -> 7 on every sheet', (el) => el.replaceAll(t.findNum, '7', { allSheets: true })],
    ['replaceAll', 'replace all whole-cell match case', (el) => el.replaceAll('TXT', 'x', { wholeCell: true, matchCase: true })],
    ['undo', 'undo', (el) => el.undo()],
    ['undo', 'undo', (el) => el.undo()],
    ['redo', 'redo', (el) => el.redo()],
  ]),
  S('view-freeze', 'frozen', ['view.freeze', 'view.gridlines', 'undo', 'redo'], () => [
    ['view.freeze', 'freeze 1 col, 1 row', (el) => { el.freezePanes(1, 1); return el.frozenPanes; }],
    ['view.freeze', 'freeze at the active cell C3', (el) => { el.selectRange('C3'); el.freezePanes(); return el.frozenPanes; }],
    ['view.freeze', 'freeze top row only', (el) => { el.freezePanes(0, 1); return el.frozenPanes; }],
    ['view.freeze', 'unfreeze', (el) => { el.freezePanes(0, 0); return el.frozenPanes; }],
    ['view.freeze', 'unfreeze again (no-op)', (el) => { el.freezePanes(0, 0); return el.frozenPanes; }],
    ['view.gridlines', 'gridlines off', (el) => { el.showGridlines = false; return el.showGridlines; }],
    ['view.gridlines', 'gridlines on', (el) => { el.showGridlines = true; return el.showGridlines; }],
    ['undo', 'undo', (el) => el.undo()],
    ['undo', 'undo', (el) => el.undo()],
    ['redo', 'redo', (el) => el.redo()],
  ]),
  S('size', 'hidden', ['size.colwidth', 'size.rowheight', 'size.autofit', 'size.hide', 'size.unhide', 'undo', 'redo'], () => [
    ['size.colwidth', 'B:C width 20', (el) => { el.selectRange('B:C'); el.setColumnWidth(20); return [el.columnWidth(2), el.columnWidth(3)]; }],
    ['size.colwidth', 'D width 3.5 (< 1 char path 0)', (el) => { el.selectRange('D1'); el.setColumnWidth(0.5); return el.columnWidth(4); }],
    ['size.rowheight', 'rows 2:3 height 30', (el) => { el.selectRange('2:3'); el.setRowHeight(30); return [el.rowHeightPt(2), el.rowHeightPt(3)]; }],
    ['size.autofit', 'autofit columns B:C', (el) => { el.selectRange('B:C'); el.autofitColumns(); return [el.columnWidth(2), el.columnWidth(3)]; }],
    ['size.autofit', 'autofit rows 2:3', (el) => { el.selectRange('2:3'); el.autofitRows(); return [el.rowHeightPt(2), el.rowHeightPt(3)]; }],
    ['size.hide', 'hide rows 4:5', (el) => { el.selectRange('4:5'); el.setHidden('rows', true); }],
    ['size.hide', 'hide column C', (el) => { el.selectRange('C:C'); el.setHidden('cols', true); }],
    ['size.unhide', 'unhide columns B:D (neighbours)', (el) => { el.selectRange('B:D'); el.setHidden('cols', false); }],
    ['size.unhide', 'unhide rows 3:6', (el) => { el.selectRange('3:6'); el.setHidden('rows', false); }],
    ['size.unhide', 'unhide with nothing hidden (no-op)', (el) => { el.selectRange('B:B'); el.setHidden('cols', false); }],
    ['undo', 'undo', (el) => el.undo()],
    ['undo', 'undo', (el) => el.undo()],
    ['redo', 'redo', (el) => el.redo()],
  ]),
  S('size-drag', 'none', ['size.drag', 'undo', 'redo'], () => {
    const drag = (kind, index, delta) => (el) => {
      const px = kind === 'col' ? el.colWidth(index) : el.rowHeight(index);
      el.resizing = { kind, index, startClient: 100, origPx: px / el.zoom };
      el.resizeBefore = el.structSnap();
      el.onResizeDragMove({ clientX: 100 + delta, clientY: 100 + delta });
      el.onResizeDragEnd();
      return [kind, index, px];
    };
    return [
      ['size.drag', 'drag column B +40 px', drag('col', 2, 40)],
      ['size.drag', 'drag row 2 +12 px', drag('row', 2, 12)],
      ['size.drag', 'drag column C to 0 (hides)', drag('col', 3, -2000)],
      ['size.drag', 'drag row 3 to 0 (hides)', drag('row', 3, -2000)],
      ['undo', 'undo', (el) => el.undo()],
      ['undo', 'undo', (el) => el.undo()],
      ['redo', 'redo', (el) => el.redo()],
    ];
  }),
  S('sheets', 'none', ['sheet.add', 'sheet.select', 'book.new', 'calc.recalc', 'undo', 'redo'], ({ t }) => [
    ['entry.text', 'A1 on the first sheet (unsaved edit travels through addSheet)', (el) => { el.selectRange('A1'); return el.setActiveCellText(t.num); }],
    ['sheet.add', 'addSheet() default name', (el) => el.addSheet()],
    ['entry.text', 'A1 on the new sheet', (el) => { el.selectRange('A1'); return el.setActiveCellText(t.pct); }],
    ['sheet.add', 'addSheet("Resumo")', (el) => el.addSheet('Resumo')],
    ['entry.formula', 'cross-sheet formula', (el) => { el.selectRange('B1'); return el.setActiveCellText(t.sum.replace('A1:A5', "'Resumo'!A1:A5").replace(/^=(\w+)\(/, '=$1(')); }],
    ['sheet.select', 'select sheet 0', (el) => { el.selectSheet(0); return el.activeSheet; }],
    ['undo', 'undo the cross-sheet formula (the stack was restarted by the addSheet before it)', (el) => el.undo()],
    ['calc.recalc', 'recalculate', (el) => el.recalculate()],
    ['book.new', 'newWorkbook()', (el) => el.newWorkbook()],
  ]),
  S('autosum', 'none', ['fn.autosum', 'fn.insert', 'undo', 'redo'], ({ t }) => [
    ['fn.autosum', 'AutoSum on a range A1:C4 (one result per column below)', (el) => { el.selectRange('A1:C4'); el.autoSum('SUM'); }],
    ['fn.autosum', 'AutoSum AVERAGE on B1:B3', (el) => { el.selectRange('B1:B3'); el.autoSum('AVERAGE'); }],
    ['fn.autosum', 'AutoSum single cell: edit state, then commit', (el) => { el.selectRange('A8'); el.autoSum('SUM'); const text = el.editor?.value; el.commitEdit(); return text; }],
    ['fn.autosum', 'AutoSum MAX single cell next to numbers', (el) => { el.selectRange('E3'); el.autoSum('MAX'); const text = el.editor?.value; el.cancelEdit(); return text; }],
    ['fn.insert', 'insertFunction then complete and commit', (el) => { el.selectRange('H1'); el.insertFunction(t.sumName); const text = el.editor?.value; el.editor.value = `${text}A1:A3)`; el.commitEdit(); return text; }],
    ['fn.insert', 'getFunctionNames (count)', (el) => el.getFunctionNames().length],
    ['undo', 'undo', (el) => el.undo()],
    ['undo', 'undo', (el) => el.undo()],
    ['redo', 'redo', (el) => el.redo()],
  ]),
  S('host-names', 'names', ['host.defineName', 'entry.formula'], ({ t }, { host }) => [
    ['host.defineName', 'upsert a workbook-level name', (el) => host.names(el, [{ name: 'Capt_One', formula: "'Sheet1'!$A$1:$A$3" }])],
    ['host.defineName', 'upsert a sheet-local name (localSheetId 0) and hidden', (el) => host.names(el, [{ name: 'Capt_Local', localSheetId: 0, formula: '1+2' }, { name: 'Capt_Hid', hidden: true, formula: '42' }])],
    ['entry.formula', 'a formula that uses the name', (el) => { el.selectRange('A1'); return el.setActiveCellText(`=${t.sumName}(Capt_One)`); }],
    ['host.defineName', 'change one name and remove another', (el) => host.names(el, [{ name: 'Capt_One', formula: "'Sheet1'!$B$1:$B$3" }], [{ name: 'Capt_Hid' }])],
  ]),
];

// workbook pool: x1 with the features each hint needs, plus the samples
const X1 = {
  none: [0, 5, 11, 22, 27, 9, 21, 14],
  merge: [1, 3, 4, 13, 16, 20, 25, 28],
  frozen: [3, 4, 6, 7, 10, 13, 15, 19],
  hidden: [2, 5, 7, 11, 12, 14, 20, 22],
  names: [1, 2, 5, 6, 7, 10, 12, 20],
};
const SAMPLES = ['demo.xlsx', 'showcase.xlsx', 'smoke.xlsx', 'merge-smoke.xlsx', 'gaps-smoke.xlsx'];
const LANGS = ['pt-BR', 'en-US'];

/** Deterministic assignment: 4 x1 workbooks (alternating languages) + 1 sample. */
function combos(si, sc) {
  const pool = X1[sc.hint] ?? X1.none;
  const out = [];
  for (let j = 0; j < 4; j++) out.push({ wb: `x1/${String(pool[(si + j * 2) % pool.length]).padStart(4, '0')}`, lang: LANGS[(si + j) % 2] });
  out.push({ wb: `samples/${SAMPLES[si % SAMPLES.length]}`, lang: LANGS[(si + 1) % 2] });
  return out;
}
function readWb(wb) {
  if (wb.startsWith('x1/')) return new Uint8Array(readFileSync(join(C.capturesDir, 'workbooks', 'x1', `x1-${wb.slice(3)}.xlsx`)));
  return C.readSample(wb.slice('samples/'.length));
}

// ---------------------------------------------------------------- host-library hooks (not element methods)
const host = {
  names(el, upserts, removals = []) { h.sheetCore.updateDefinedNames(el.pkg, upserts, removals); return `names ${upserts.length}+${removals.length}`; },
};

// ---------------------------------------------------------------- one script
const w = new CaptureWriter(['Sheet edit operations (frontier C5a): see core/crates/uo-xlsx/js/capture-edit.mjs and core/docs/modules/uo-xlsx.md "C5a — as built"']);
w.meta('ts-root-commit', h.commit);
w.meta('ts-root-state', C.tsClean(root));
w.meta('tz', process.env.TZ);
w.meta('clock', `Date frozen at ${new RealDate(FIXED).toISOString()}; Math.random = xorshift32(sha256(script key)[0..8]) / 2^32`);
w.meta('canon', 'canon-json.mjs of uo-pptx: sorted keys, shortest round-trip numbers, -0 kept, Map as {"$map"}');
const side = [];
const counts = new Map(); // op -> { steps, wbs:Set, langs:Set }
const unsupported = [];

function snapShape(s) { return `${s.sheet}:${s.cells ? s.cells.length : '-'}:${s.struct ? 'S' : ''}`; }
function stackText(stack) {
  return `${stack.length}:${stack.map((e) => `${e.label}[${e.before.map(snapShape).join(',')}>${e.after.map(snapShape).join(',')}]`).join(' ')}`;
}
function entryText(e) {
  const conv = (s) => ({ sheet: s.sheet, cells: s.cells, struct: s.struct });
  return canonJson({ label: e.label, before: e.before.map(conv), after: e.after.map(conv) });
}

function recordScript(sc, si, c) {
  const wbKey = c.wb;
  const rkey = `edit/${sc.id}/${wbKey}/${c.lang}`;
  const bytes = readWb(wbKey);
  const sx = (facet, text) => side.push(`${rkey}\t${facet}\t${text}`);
  w.set(rkey, 'input', `${bytes.length}:${sha(bytes).slice(0, 16)}`);
  w.set(rkey, 'scenario', sc.id);
  w.set(rkey, 'ops', sc.ops.join(','));
  rng = new Rng((parseInt(sha(rkey).slice(0, 8), 16) >>> 0) || 1);
  const el = h.newSheet({ 'formula-locale': c.lang });
  try { quiet(() => el.loadXlsx(bytes)); } catch (e) { w.set(rkey, 'open', `err:${C.oneLine(e?.message ?? e)}`); el.remove(); unsupported.push(`${rkey}: open ${e?.message}`); return; }
  const base = partsOf(bytes);
  let prevParts = base;
  const prevSt = new Map(), prevText = new Map(), prevXml = new Map();
  let prevTop = null;

  // facets are carried forward: `put` writes a facet only when its text differs from the previous step's (the first step, `o`, always)
  const put = (p, facet, text) => { if (prevText.get(`f:${facet}`) === text) return false; prevText.set(`f:${facet}`, text); w.set(rkey, `${p}.${facet}`, text); return true; };
  const dg = (s) => `${Buffer.byteLength(s)}:${sha(s).slice(0, 16)}`;
  const snapshot = (p) => {
    const first = p === 'o';
    const gs = el.getSelection();
    put(p, 'sel', `${el.activeSheet}|${gs.col},${gs.row}|${gs.c1},${gs.r1},${gs.c2},${gs.r2}`);
    w.set(rkey, `${p}.undo`, stackText(el.undoStack));
    w.set(rkey, `${p}.redo`, stackText(el.redoStack));
    const top = el.undoStack[el.undoStack.length - 1] ?? null;
    if (top) {
      const t = entryText(top);
      w.set(rkey, `${p}.utop`, dg(t));
      if (top !== prevTop && t.length <= 4000) sx(`${p}.utop`, t);
    } else w.set(rkey, `${p}.utop`, '-');
    prevTop = top;
    const list = (s) => [...s].sort((x, y) => x - y).join(',') || '-';
    put(p, 'dirty', `dirty=${el.isDirty()} dirtySheets=${list(el.dirtySheets)} merge=${list(el.mergeDirty)} view=${list(el.viewDirty)}`);
    const fp = el.frozenPanes;
    put(p, 'sheets', `${el.getSheetNames().join('|')} zoom=${el.getZoom()} frozen=${fp.cols},${fp.rows} grid=${el.showGridlines} clip=${el.hasClipboard} undo=${el.canUndo()} redo=${el.canRedo()}`);
    (el.views ?? []).forEach((v, i) => {
      const st = storeOf(v.worksheet);
      const stText = [...st.lines.values()].join('\n');
      const prev = prevSt.get(i);
      if (put(p, `st${i}`, `${st.lines.size}:${sha(stText).slice(0, 16)}`) && !first) for (const l of delta(prev?.lines, st.lines)) sx(`${p}.st${i}`, l);
      prevSt.set(i, { lines: st.lines });
      const { rows, ...rest } = v.worksheet;
      const mt = canonJson(rest);
      if (put(p, `ws${i}`, dg(mt)) && !first) sx(`${p}.ws${i}`, mt);
      if (put(p, `rw${i}`, listDig(st.rowMeta).slice(0, 20)) && !first) for (const l of st.rowMeta) sx(`${p}.rw${i}`, l);
    });
    try {
      const lines = quiet(() => el.debugCellDisplays()).map((d) => `${d.ref}\t${d.display}`);
      const ai = el.activeSheet;
      const map = new Map(lines.map((l) => [l.split('\t')[0], JSON.stringify(l)]));
      const pm = prevText.get(`dmap${ai}`);
      if (put(p, `d${ai}`, `${lines.length}:${sha(lines.join('\n')).slice(0, 16)}`) && !first) for (const l of delta(pm, map)) sx(`${p}.d${ai}`, l);
      prevText.set(`dmap${ai}`, map);
    } catch (e) { w.set(rkey, `${p}.d${el.activeSheet}`, `err:${C.oneLine(e?.message ?? e)}`); }
    try {
      const saved = quiet(() => el.saveXlsx());
      const parts = partsOf(saved);
      const lines = [...parts].map(([n, dt]) => `${n} ${dt.length} ${sha(dt).slice(0, 32)}`);
      w.set(rkey, `${p}.save`, listDig(lines).slice(0, 24));
      const diff = (ref) => [...parts].filter(([n, dt]) => !ref.has(n) || !ref.get(n).equals(dt)).map(([n]) => n);
      const gone = (ref) => [...ref.keys()].filter((n) => !parts.has(n));
      w.set(rkey, `${p}.chg`, [...diff(base), ...gone(base).map((n) => `-${n}`)].join(' ') || '-');
      const chgp = diff(prevParts);
      w.set(rkey, `${p}.chgp`, [...chgp, ...gone(prevParts).map((n) => `-${n}`)].join(' ') || '-');
      // which structural XML the saved package carries (sheet parts + workbook.xml): a reachability audit for the C1b open items
      {
        const feat = { merge: 0, pane: 0, colDef: 0, colHidden: 0, rowHt: 0, rowHidden: 0, rowStyle: 0, gridOff: 0, defName: 0, calcPr: 0, sheets: 0 };
        for (const [n, dt] of parts) {
          const x = dt.toString('utf8');
          if (/^xl\/worksheets\/sheet\d+\.xml$/.test(n)) {
            feat.merge += (x.match(/<mergeCell[ >]/g) ?? []).length; feat.pane += (x.match(/<pane[ />]/g) ?? []).length;
            feat.colDef += (x.match(/<col[ />]/g) ?? []).length; feat.colHidden += (x.match(/<col [^>]*hidden=/g) ?? []).length;
            feat.rowHt += (x.match(/<row [^>]*ht=/g) ?? []).length; feat.rowHidden += (x.match(/<row [^>]*hidden=/g) ?? []).length;
            feat.rowStyle += (x.match(/<row [^>]*customFormat=/g) ?? []).length; feat.gridOff += (x.match(/showGridLines="(0|false)"/g) ?? []).length;
          } else if (n === 'xl/workbook.xml') { feat.defName += (x.match(/<definedName[ >]/g) ?? []).length; feat.calcPr += (x.match(/<calcPr[ />]/g) ?? []).length; feat.sheets += (x.match(/<sheet[ />]/g) ?? []).length; }
        }
        put(p, 'xmlfeat', Object.entries(feat).map(([k, v]) => `${k}=${v}`).join(','));
      }
      if (first) for (const l of lines) sx(`${p}.save`, l);
      else for (const n of chgp) if (lines.length) sx(`${p}.save`, `+${lines.find((l) => l.startsWith(`${n} `))}`);
      if (XML) for (const n of chgp) {
        const dt = parts.get(n);
        if (!/\.(xml|rels)$/.test(n)) continue;
        if (/^xl\/(drawings|charts|media)\//.test(n) || dt.length > 60_000) continue;
        // text diff against the part's previous version (the input's on first change): common prefix/suffix + the middle
        const was = (prevXml.get(n) ?? base.get(n))?.toString('utf8') ?? '';
        const now = dt.toString('utf8');
        let pre = 0; const m = Math.min(was.length, now.length);
        while (pre < m && was.charCodeAt(pre) === now.charCodeAt(pre)) pre++;
        let suf = 0;
        while (suf < m - pre && was.charCodeAt(was.length - 1 - suf) === now.charCodeAt(now.length - 1 - suf)) suf++;
        sx(`${p}.save:${n}`, `${pre},${suf},${JSON.stringify(now.slice(pre, now.length - suf))}`);
      }
      for (const [n, dt] of parts) if (chgp.includes(n)) prevXml.set(n, dt);
      prevParts = parts;
    } catch (e) { w.set(rkey, `${p}.save`, `err:${C.oneLine(e?.message ?? e)}`); }
    try { const stl = el.styles; put(p, 'xf', `${stl.cellXfs.length}:${stl.numFmts.size}`); } catch { /* no styles */ }
  };

  snapshot('o');
  const steps = sc.steps({ t: T[c.lang] }, { host });
  steps.forEach(([op, desc, run], k) => {
    const p = `e${k}`;
    w.set(rkey, p, `${op} | ${desc}`);
    let res;
    try { res = quiet(() => run(el)); res = res === undefined ? '-' : JSON.stringify(res, jsonSets); } catch (e) { res = `throw:${C.oneLine(e?.message ?? e)}`; unsupported.push(`${rkey} e${k} ${op}: ${e?.message}`); }
    w.set(rkey, `${p}.r`, C.oneLine(res ?? '-', 240));
    const cn = counts.get(op) ?? { steps: 0, wbs: new Set(), langs: new Set(), scripts: new Set() };
    cn.steps++; cn.wbs.add(wbKey); cn.langs.add(c.lang); cn.scripts.add(rkey);
    counts.set(op, cn);
    snapshot(p);
  });
  el.remove();
}

const t0 = performance.now();
let nScripts = 0;
scenarios.forEach((sc, si) => {
  for (const c of combos(si, sc)) {
    const k = `edit/${sc.id}/${c.wb}/${c.lang}`;
    if (only && !k.includes(only)) continue;
    recordScript(sc, si, c);
    nScripts++;
  }
});
const opsSummary = [...counts].sort((x, y) => (x[0] < y[0] ? -1 : 1)).map(([op, cn]) => `${op}=${cn.steps}/${cn.wbs.size}/${[...cn.langs].sort().join('+')}`);
w.meta('ops', opsSummary.join(' '));
w.meta('scripts', nScripts);
const out = a.opt('--out', join(C.capturesDir, 'sheet-edit.capture'));
mkdirSync(dirname(out), { recursive: true });
w.write(out);
{
  const gz = createGzip({ level: 9 });
  const done = new Promise((res, rej) => gz.pipe(createWriteStream(out.replace(/\.capture$/, '.side.gz'))).on('finish', res).on('error', rej));
  for (let i = 0; i < side.length; i += 10000) gz.write(side.slice(i, i + 10000).join('\n') + '\n');
  gz.end();
  await done;
}
console.log(`sheet edit: ${nScripts} scripts, ${[...counts.values()].reduce((s, c) => s + c.steps, 0)} steps in ${((performance.now() - t0) / 1000).toFixed(1)} s -> ${out}`);
{
  for (const l of opsSummary) console.log(`  ${l}`);
  if (unsupported.length) { console.log(`  throws (${unsupported.length}):`); for (const u of unsupported.slice(0, 40)) console.log(`    ${u}`); }
}
