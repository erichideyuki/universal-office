// Parity capture group `a5` (frontier A5, record core/docs/modules/uo-calc.md "A5 — as built"): the formula-language
// layer, the legacy `@` rewrite, the file form, the volatile test and the add-in resolution of the frozen TS, called
// directly on the calc dist. Deterministic (seeded); one JSON object per line, gzip.
//
//   node core/crates/uo-calc/js/capture-a5.mjs                  # CI -> tests/captures/a5.capture.gz
//   node core/crates/uo-calc/js/capture-a5.mjs --big --out core/target/sheet-cap/a5-big.capture.gz
//
// Records (`k`):
//   tl / fl   toLocal / fromLocal: {t: text, o: options (lang as an id), r: result}; every catalogue function name
//             both ways in pt-BR and en-US (+ file prefixes, + accents stripped and lower case typed), every error,
//             boolean, separator set (decimal/list/array), R1C1 (relative/absolute, both letter sets), structured
//             references (keywords, @, column ranges, inTable), LET/LAMBDA parameters (_xlpm/_xlop), _xleta, the
//             operator functions (SINGLE, ANCHORARRAY, _TRO_*), quoted sheet names, external books, names (with the
//             `names` option), unknown functions (_xludf), white space and line breaks; ids without a table (xx-YY,
//             de-DE: the host's Intl decimal is in @intl); seeded mutations of all of them
//   lg        legacyToDynamic(parseFormula(f, at)) with the deterministic host rule of @legacy-host: {f, at, ui, file,
//             pos} (the dynamic tree printed in the screen and file forms, the implicit positions sorted)
//   ff        toFileFormula(parseFormula(f, at), row, col, same host rule): {f, at, text, dynamic}
//   vo        hasVolatileFunction(f): {f, v}
//   ad        resolveAddinCalls(parseFormula(f), lang): {f, lang, file}
//   ce / cf / ln  canonicalError / canonicalFunctionName / localFunctionName over typed texts
process.env.TZ = 'UTC';
const { join, dirname } = await import('node:path');
const { mkdirSync, writeFileSync, readFileSync, readdirSync } = await import('node:fs');
const { gzipSync, gunzipSync } = await import('node:zlib');
const { pathToFileURL } = await import('node:url');
const { Rng } = await import('../../uo-testkit/js/testkit.mjs');
const C = await import('./lib/common.mjs');

const a = C.args();
const root = C.tsRoot(a);
const big = a.flag('--big');
const out = a.opt('--out', join(C.capturesDir, 'a5.capture.gz'));

const base = join(root, 'sheet', 'packages', 'calc', 'dist');
const imp = (p) => import(pathToFileURL(join(base, p)).href);
const calc = await imp('index.js');
const IM = await imp('implicit.js');
const FF = await imp('file-form.js');
const OR = await imp('open-recalc.js');
const AD = await imp('addin.js');
const TR = await imp('i18n/translate.js');
const CAT = await imp('i18n/catalog.js');

const rng = new Rng(0xa5a5_0005);
const pick = (xs) => xs[rng.below(xs.length)];
const lines = [];
const counts = new Map();
const rec = (k, o) => { counts.set(k, (counts.get(k) ?? 0) + 1); lines.push(JSON.stringify({ k, ...o })); };
// the host's Intl decimal of the ids without a table (decision C1: the shell's regionalDecimal); an unknown id
// falls back to the machine's default locale (pt-BR here: 'xx-YY' → ',')
const INTL = Object.fromEntries(['xx-YY', 'de-DE', 'ja-JP'].map((id) => [id, new Intl.NumberFormat(id).formatToParts(1.5).find((p) => p.type === 'decimal')?.value]));

// ---------------------------------------------------------------- translator inputs
const names = CAT.catalog().map((e) => e.name);
const texts = new Set();
const addT = (t) => texts.add(t);
for (const n of names) {
  const e = CAT.catalogEntry(n);
  const args = Array.from({ length: Math.min(Math.max(e.minArgs, 1), 3) }, (_, i) => ['A1', '1.5', '"x"', 'B2:C3'][i % 4]).join(',');
  addT(`=${n}(${args})`);
  addT(`=${e.prefix}${n}(${args})`);
  addT(`=${n}`);
}
const ERRS = ['#NULL!', '#DIV/0!', '#VALUE!', '#REF!', '#NAME?', '#NUM!', '#N/A', '#GETTING_DATA', '#SPILL!', '#CALC!', '#FIELD!', '#BLOCKED!', '#CONNECT!', '#UNKNOWN!', '#BUSY!', '#PYTHON!'];
for (const e of ERRS) { addT(`=IF(A1=${e},1,${e})`); addT(`={1,${e};TRUE,"a"}`); addT(`=ISERROR(${e.toLowerCase()})`); }
const fixed = [
  '=SUM(A1:B2,1.25E+3,-0.5,.5)', '={1.5,2.25;3,4}', '={TRUE,FALSE;"a""b",#N/A}', '=IF(TRUE,"x","y")&"""q"""',
  "='My Sheet'!A1+'It''s'!B2", "=Sheet1!A1:Sheet1!B2", "=[1]Sheet1!A1+[Book.xlsx]S!$B$2", '=Tabela1[[#This Row],[Qtd]]*2',
  '=SUM(Tabela1[Qtd])', '=SUBTOTAL(109,Tabela1[Qtd])', '=Tabela1[[#All],[Qtd]:[Valor]]', '=Tabela1[#Headers]', '=Tabela1[]',
  '=Tabela1[[#Totals],[Valor]]', '=Tabela1[@Qtd]', '=Tabela1[@[Col A]]', '=[@Qtd]+[@[Valor]]', '=SUM(Tabela1[@[Qtd]:[Valor]])',
  '=_xlfn.LET(_xlpm.x,A1,_xlpm.y,2,_xlpm.x*_xlpm.y)', '=LET(x,1,y,x+1,y*2)', '=LAMBDA(a,b,a+b)(1,2)', '=_xlfn.LAMBDA(_xlpm.a,_xlop.b,_xlpm.a)(1)',
  '=LAMBDA(taxa,taxa*2)(3)', '=LET(rate,0.1,rate*A1)', '=_xlfn.GROUPBY(A1:A3,B1:B3,_xleta.SUM)', '=GROUPBY(A1:A3,B1:B3,SUM)',
  '=_xlfn.SINGLE(A1:A3)', '=_xlfn.ANCHORARRAY(A1)', '=SUM(_xlfn.ANCHORARRAY(A1))', '=_xlfn._TRO_ALL(A1:B5)', '=_xlfn._TRO_TRAILING(A:A)',
  '=_xlfn._TRO_LEADING(A1:A9)+1', '=_xlfn.SINGLE(_xlfn._TRO_ALL(A1:B5))', '=@A1:A3', '=A1#', '=A1:.B5', '=A1.:B5', '=A1.:.B5',
  '=MYUDF(1)', '=_xludf.SUM(1)', '=_xll.ISEVEN(A1)', '=_xlfn._xlws.SORT(A1:A3)', '=_xlfn.XLOOKUP(1,A1:A3,B1:B3)',
  '=SUM(A1:A3)\n+1', '=SUM(A1:A3)\r\n+1', '= SUM( A1 , B1 )', '=A1 B1', '=(A1:A3,C1)', '=Taxa*2', '=taxa+RATE(1,2,3)', '=TRUE+FALSE',
  '=R1C1', '=L1C1+L2C3', '=r1c1', '=A1:XFD1048576', '=$A:$C', '=2:5', '=-A1%', '=1E+21', '=1e-7', '=12345678901234567890',
  '=INDIRECT("R1C1",FALSE)', '="a,b;c"&{1,2}', '=SUM({1,2,3})', '=IFERROR(1/0,"x")', '=CONCAT("a";"b")',
];
for (const t of fixed) addT(t);
const r1c1 = ['=R1C1', '=R[1]C[-1]', '=RC', '=R[-2]C3+R5C', '=SUM(R1C1:R[2]C[3])', '=L1C1+L[1]C[-1]', '=LC', '=R1C1.:R3C3', '=Sheet1!R1C1', '=R[+1]C'];
// pt-BR typed texts (local form) for fromLocal
const ptTyped = [];
for (const t of [...texts]) {
  let l;
  try { l = TR.toLocal(t, { lang: 'pt-BR' }); } catch { continue; }
  ptTyped.push(l);
  ptTyped.push(l.toLowerCase());
  ptTyped.push(l.normalize('NFD').replace(/\p{M}/gu, '').normalize('NFC'));
}
ptTyped.push('=SOMA(1;2)', '=SOMA(1,5;2)', '=MÁXIMO(A1:A3)', '=MAXIMO(A1:A3)', '=CONT.NUM(A1:A3)', '=_xlfn.PROCX(1;A1:A3;B1:B3)',
  '=SE(VERDADEIRO;"a";"b")', '={1\\2;3\\4}', '={1,5\\2,5}', '=#N/D+#VALOR!', '=Tabela1[[#Esta Linha];[Qtd]]', '=Tabela1[[#Tudo];[Qtd]]',
  '=LAMBDA(taxa;taxa*2)(3)', '=LET(x;1;y;x+1;y)', '=AGRUPARPOR(A1:A3;B1:B3;SOMA)', "='Aba 1'!A1", '=SUM(1;2)', '=L1C1', '=SOMASEQUENCIA(1;2;3;4)');

const OPTS = [
  { lang: 'pt-BR' }, { lang: 'en-US' }, {}, { lang: 'pt-BR', r1c1: true }, { lang: 'en-US', r1c1: true },
  { lang: 'pt-BR', decimal: '.', list: ',' }, { lang: 'pt-BR', decimal: ',' }, { lang: 'en-US', decimal: ',', list: ';' },
  { lang: 'xx-YY' }, { lang: 'de-DE' }, { lang: 'pt-BR', inTable: 'Tabela1' }, { lang: 'en-US', inTable: 'tabela1' },
  { lang: 'pt-BR', names: ['Taxa', 'rate'] }, { lang: 'pt-BR', arrayCol: ';', arrayRow: '|' },
];
const FROM_OPTS = OPTS.map((o) => ({ ...o })).concat([{ lang: 'pt-BR', filePrefixes: true }, { lang: 'en-US', filePrefixes: true },
  { lang: 'pt-BR', filePrefixes: true, inTable: 'Tabela1' }, { lang: 'pt-BR', r1c1: true, filePrefixes: true }]);

function mutate(t) {
  const ops = [
    () => t.slice(0, rng.below(t.length + 1)),
    () => { const i = rng.below(t.length + 1); return t.slice(0, i) + pick(['(', ')', '"', "'", '[', ']', '{', '}', ';', ',', '#', '@', ':', '.', ' ', '\\', 'Ç', '́', '\ud83d']) + t.slice(i); },
    () => t.toUpperCase(),
    () => t.replace(/[,;]/g, () => pick([',', ';', '\\'])),
  ];
  return pick(ops)();
}

const toL = [...texts, ...r1c1];
const n0 = big ? 20 : 3;
for (const t of toL) {
  for (const o of OPTS) {
    if (!o.r1c1 && r1c1.includes(t) && rng.below(2)) continue;
    rec('tl', { t, o, r: TR.toLocal(t, o) });
  }
}
for (const t of [...ptTyped, ...toL]) {
  for (const o of FROM_OPTS) rec('fl', { t, o, r: TR.fromLocal(t, o) });
}
for (let i = 0; i < toL.length * n0; i++) {
  const t = mutate(pick([...toL, ...ptTyped]));
  const o = pick(FROM_OPTS);
  const k = rng.below(2) ? 'tl' : 'fl';
  rec(k, { t, o, r: k === 'tl' ? TR.toLocal(t, o) : TR.fromLocal(t, o) });
}
// name functions
for (const l of ['pt-BR', 'en-US', 'xx-YY']) {
  for (const n of names) {
    rec('ln', { t: n, lang: l, r: TR.localFunctionName(n, l) });
    rec('ln', { t: `_xlfn.${n.toLowerCase()}`, lang: l, r: TR.localFunctionName(`_xlfn.${n.toLowerCase()}`, l) });
    const loc = TR.localFunctionName(n, l);
    for (const typed of [loc, loc.toLowerCase(), loc.normalize('NFD').replace(/\p{M}/gu, '').normalize('NFC'), n]) {
      rec('cf', { t: typed, lang: l, r: TR.canonicalFunctionName(typed, l) ?? null });
    }
  }
  for (const e of ERRS) for (const typed of [TR.localizeError(e, l), ` ${TR.localizeError(e, l).toLowerCase()} `, e]) {
    rec('ce', { t: typed, lang: l, r: TR.canonicalError(typed, l) ?? null });
  }
}

// ---------------------------------------------------------------- formulas of the bank and the generated workbooks
const formulas = [];
const seen = new Set();
const addF = (f, at) => { if (typeof f !== 'string' || seen.has(f)) return; seen.add(f); formulas.push({ f, at: at ?? [0, 0] }); };
for (const file of readdirSync(C.oracleDir).filter((x) => x.endsWith('.cases.json')).sort()) {
  const j = JSON.parse(readFileSync(join(C.oracleDir, file), 'utf8'));
  for (const c of Array.isArray(j) ? j : j.cases) {
    if (c.f) addF(c.f.replace(/^=/, ''), [9, 3]);
    for (const [, v] of Object.entries(c.cells ?? {})) if (v && typeof v === 'object' && 'f' in v) addF(String(v.f).replace(/^=/, ''), [5, 5]);
  }
}
try {
  const side = gunzipSync(readFileSync(join(C.capturesDir, 'calc-order.side.gz'))).toString('utf8');
  for (const line of side.split('\n')) {
    const p = line.split('\t');
    if (p[1] !== 'wb') continue;
    const wb = JSON.parse(p.slice(2).join('\t'));
    for (const sh of wb.sheets) for (const [r, c, cell] of sh.cells) if (cell.f?.text) addF(cell.f.text, [r, c]);
  }
} catch { /* no sidecar */ }
// the measured classes: every catalogue function over ranges, names, tables, anchors
const shapes = ['A1:A3', 'A1', 'Nome', 'Tabela1[Qtd]', 'A1#', '(A1:A3,B1)', 'A1:A3*2', '{1,2}', 'INDEX(A1:B3,0,1)', 'OFFSET(A1,0,0,2)', 'B1:B3 B2:C2'];
for (const n of names) {
  const e = CAT.catalogEntry(n);
  const k = Math.min(Math.max(e.minArgs, 1), 5);
  for (let s = 0; s < (big ? 6 : 2); s++) addF(`${n}(${Array.from({ length: k }, () => pick(shapes)).join(',')})`, [4, 4]);
  addF(`SUM(${n}(${Array.from({ length: k }, () => pick(shapes)).join(',')}))`, [4, 4]);
}
for (const f of ['IF(A1#>1,1,2)', 'TYPE(A1#)', 'TYPE(A1:A3)', 'SUMPRODUCT(IF(A1:A3>1,1,0))', 'SUMPRODUCT(SUM(A1:A3*1))', 'INDEX(A1:A3,MATCH(1,B1:B3,0))',
  'INDEX(A1:A3,0)', 'INDEX((A1:A3,B1:B3),1,1,2)', 'XLOOKUP(1,A1:A3,B1:B3)', 'XLOOKUP(1,A1:A3,B1:C3)', 'XLOOKUP(1,A1:C1,A2:C2)', 'ROW(A1:A3)', 'ROW()',
  'OFFSET(A1,0,0)', 'CELL("row",A1:A3)', 'Nome2(1)', 'LAMBDA(x,x)(A1:A3)', 'MYUDF(A1:A3)', '_xll.ISEVEN(A1:A3)', '_xludf.F(A1:A3)', 'SUM(SIGN(FREQUENCY(A1:A3,B1:B3)))',
  'SIGN(FREQUENCY(A1:A3,B1:B3))', 'Tabela1', 'A1:A3', '-A1:A3', '((A1:A3))', 'A1:A3+B1:B3', 'UPPER({"a";"b"})', 'SUM({1,2})', 'LET(x,A1:A3,x)', 'IF(1,A1,A1:A2)',
  'IFERROR(1/A1:A3,0)', 'CHOOSE(1,A1:A3)', 'ISFORMULA(A1:A3)', 'COLUMN(B1:C1)', 'VLOOKUP(A1:A2,A1:B3,2,0)', 'Nome+1', 'NomeUnico*2', 'irr_guess']) addF(f, [6, 6]);
const hostRule = 'nameIsMulti(n) = n.toUpperCase() !== "NOMEUNICO" && n.toUpperCase() !== "IRR_GUESS"; isName(n) = n.toUpperCase().startsWith("NOME")';
const host = { nameIsMulti: (n) => n.toUpperCase() !== 'NOMEUNICO' && n.toUpperCase() !== 'IRR_GUESS', isName: (n) => n.toUpperCase().startsWith('NOME') };
const take = big ? formulas : formulas.filter((_, i) => i % 3 === 0 || i >= formulas.length - 1500);
for (const { f, at } of take) {
  let node;
  try { node = calc.parseFormula(f, { row: at[0], col: at[1] }); } catch { continue; }
  let ex;
  try { ex = IM.legacyToDynamic(node, host); } catch (e) { rec('lg', { f, at, throw: String(e?.constructor?.name) }); continue; }
  const pos = IM.implicitPositions(ex).sort((x, y) => x - y);
  rec('lg', { f, at, ui: calc.printFormula(ex, { row: at[0], col: at[1], ui: true }), file: calc.printFormula(ex, { row: at[0], col: at[1] }), pos });
  let dynNode;
  try { dynNode = calc.parseFormula(calc.printFormula(node, { row: at[0], col: at[1], ui: true }), { row: at[0], col: at[1] }); } catch { dynNode = node; }
  const ff = FF.toFileFormula(dynNode, at[0], at[1], host);
  rec('ff', { f: calc.printFormula(dynNode, { row: at[0], col: at[1] }), at, text: ff.text, dynamic: ff.dynamic });
  rec('vo', { f, v: OR.hasVolatileFunction(f) });
}
for (const f of ['_xll.ISEVEN(A1)', '_xll.ÉPAR(A1)', '_xll.BESSELJ(2.5,1)', '_xll.FRAÇÃOANO(A1,B1)', '_xll.YEARFRAC(A1,B1)+_xll.CONVERTER(1,"m","cm")',
  'SUM(_xll.ISODD(A1),_XLL.éímpar(B1))', '_xll.MINHAUDF(1)', 'LAMBDA(x,_xll.ÉPAR(x))(2)', '-(_xll.DELTA(1))']) {
  for (const lang of ['pt-BR', 'en-US', 'xx-YY']) {
    const n = calc.parseFormula(f);
    rec('ad', { f, lang, file: calc.printFormula(AD.resolveAddinCalls(n, lang), {}) });
  }
}
for (const f of ['NOW()', '"NOW()"&A1', 'cell("row",A1)', 'RANDBETWEEN (1,2)', 'XRAND(1)', '_RAND()', '1RAND()', 'SUM(RAND())', 'OFFSET(A1,0,0', 'SUM(1', 'info("x")',
  'LET(x,INDIRECT("A1"),x)', 'TODAY  ()', 'RANDARRAY(2)', 'MYNOW()', 'NOW', 'A1+RANDBETWEEN(1,2)']) rec('vo', { f, v: OR.hasVolatileFunction(f) });

const head = { k: 'meta', ts: C.tsCommit(root), clean: C.tsClean(root), intl: INTL, legacyHost: hostRule, counts: Object.fromEntries([...counts].sort()), big };
const text = [JSON.stringify(head), ...lines].join('\n') + '\n';
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, gzipSync(Buffer.from(text, 'utf8'), { level: 9 }));
console.log(`wrote ${out}: ${lines.length} records ${JSON.stringify(head.counts)} ${(gzipSync(Buffer.from(text)).length / 1024).toFixed(0)} KiB`);
