// Generates core/crates/uo-calc/src/i18n/tables.rs (frontier A5, record core/docs/modules/uo-calc.md "A5"): the
// static data of the formula-language layer of the frozen TS — the Excel function catalogue
// (`i18n/catalog-data.ts`: id, file prefix, argument counts, reserved and unavailable names, operator functions), the
// registered language tables (`i18n/languages.ts` EN_US_LANG + `i18n/langs/pt-BR.ts`) with the lookup maps the TS
// builds at run time (`translate.ts` `langData`: the folded local name → canonical map, the error and structured-
// reference keyword maps), the list of the Office interface languages (`EXCEL_LANGUAGES`) and the measured implicit-
// intersection classes (`implicit-classes.ts`).
//
// The run-time maps are built HERE with the TS code's own JavaScript semantics (`toLocaleUpperCase(id)`,
// `normalize('NFD')`, `\p{M}`, Map insertion order), so the Rust side only folds the typed word and searches.
//
//   node core/crates/uo-calc/js/gen-i18n-tables.mjs          # rewrites src/i18n/tables.rs
//   node core/crates/uo-calc/js/gen-i18n-tables.mjs --check  # fails when the committed file differs
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
const C = await import('./lib/common.mjs');

const a = C.args();
const root = C.tsRoot(a);
const base = join(root, 'sheet', 'packages', 'calc', 'dist');
const imp = (p) => import(pathToFileURL(join(base, p)).href);
const data = await imp('i18n/catalog-data.js');
const langs = await imp('i18n/languages.js');
const ptmod = await imp('i18n/langs/pt-BR.js');
const implicit = await imp('implicit-classes.js');

const ERROR_CODES = ['#NULL!', '#DIV/0!', '#VALUE!', '#REF!', '#NAME?', '#NUM!', '#N/A', '#GETTING_DATA',
  '#SPILL!', '#CALC!', '#FIELD!', '#BLOCKED!', '#CONNECT!', '#UNKNOWN!', '#BUSY!', '#PYTHON!'];
const ERR_VARIANT = {
  '#NULL!': 'Null', '#DIV/0!': 'Div0', '#VALUE!': 'Value', '#REF!': 'Ref', '#NAME?': 'Name', '#NUM!': 'Num', '#N/A': 'NA',
  '#GETTING_DATA': 'GettingData', '#SPILL!': 'Spill', '#CALC!': 'Calc', '#FIELD!': 'Field', '#BLOCKED!': 'Blocked',
  '#CONNECT!': 'Connect', '#UNKNOWN!': 'Unknown', '#BUSY!': 'Busy', '#PYTHON!': 'Python',
};
const KW_KEYS = ['all', 'data', 'headers', 'totals', 'thisRow'];
const KW_VARIANT = { all: 'All', data: 'Data', headers: 'Headers', totals: 'Totals', thisRow: 'ThisRow' };

const q = (s) => JSON.stringify(s);
/** Code-point order (= UTF-16 order for these BMP texts below U+D800; = the byte order of the Rust &str). */
const cpCmp = (x, y) => {
  const ax = [...x].map((c) => c.codePointAt(0)), ay = [...y].map((c) => c.codePointAt(0));
  for (let i = 0; i < Math.min(ax.length, ay.length); i++) if (ax[i] !== ay[i]) return ax[i] - ay[i];
  return ax.length - ay.length;
};
const checkBmp = (s) => { for (const c of s) if (c.codePointAt(0) >= 0xd800) throw new Error(`non-BMP/surrogate text in a table: ${q(s)}`); };

// ------------------------------------------------------------------ catalogue
const rows = data.SHEET_FUNCTIONS;
const catLines = rows.map(([name, id, p, min, max, step = 1, small]) => {
  checkBmp(name);
  const op = data.OPERATOR_FUNCTIONS[name];
  return `    CatalogEntry { name: ${q(name)}, id: ${id}, prefix: ${q(data.FILE_PREFIXES[p] ?? '')}, min_args: ${min}, max_args: ${max}, arg_step: ${step}, small_counts: &[${(small ?? []).join(', ')}], operator: ${op ? `Some(${q(op)})` : 'None'} },`;
});
const byName = rows.map((r, i) => [r[0], i]).sort((x, y) => cpCmp(x[0], y[0])).map(([, i]) => i);
const sortedList = (l) => [...l].sort(cpCmp);
const reserved = sortedList(data.RESERVED_FUNCTIONS);
const unavailable = sortedList(data.UNAVAILABLE_FUNCTIONS);
const opFns = Object.entries(data.OPERATOR_FUNCTIONS).sort((x, y) => cpCmp(x[0], y[0]));

// ------------------------------------------------------------------ languages
const stripMarks = (s) => s.normalize('NFD').replace(/\p{M}/gu, '').normalize('NFC');
const catalogNames = rows.map((r) => r[0]);

/** `langData(lang)` of translate.ts, as data. */
function langData(lang) {
  const fold = (s) => s.toLocaleUpperCase(lang.id);
  const fnIn = new Map();
  for (const name of catalogNames) if (!lang.functions[name]) fnIn.set(fold(name), name);
  for (const [canon, local] of Object.entries(lang.functions)) fnIn.set(fold(local), canon);
  for (const [k, canon] of [...fnIn]) {
    const bare = stripMarks(k);
    if (bare !== k && !fnIn.has(bare)) fnIn.set(bare, canon);
  }
  const errOut = (code) => lang.errors?.[code] ?? code;
  const errIn = ERROR_CODES.map((c) => [fold(errOut(c)), c]).sort((x, y) => y[0].length - x[0].length);
  const table = { ...langs.EN_US_LANG.table, ...lang.table };
  const kwIn = KW_KEYS.map((k) => [fold(table[k]), k]);
  // the TS Map keeps the LAST insertion of a duplicate key: so does `new Map(entries)`
  const kwMap = new Map(kwIn);
  return { fold, fnIn, errIn, kwMap, table };
}

function langBlock(constName, lang) {
  const d = langData(lang);
  const fns = Object.entries(lang.functions).sort((x, y) => cpCmp(x[0], y[0]));
  for (const [k, v] of fns) { checkBmp(k); checkBmp(v); }
  const fnIn = [...d.fnIn].sort((x, y) => cpCmp(x[0], y[0]));
  for (const [k] of fnIn) checkBmp(k);
  const errs = ERROR_CODES.filter((c) => lang.errors?.[c] !== undefined).map((c) => `(ErrorCode::${ERR_VARIANT[c]}, ${q(lang.errors[c])})`);
  const opt = (v) => (v === undefined ? 'None' : `Some(${q(v)})`);
  const r1 = lang.r1c1;
  const arr = lang.array;
  const fmt = lang.format;
  const reg = lang.regional;
  const tbl = lang.table ?? {};
  return `/// \`${lang.id}\` (${lang.source}).
pub static ${constName}: FormulaLanguage = FormulaLanguage {
    id: ${q(lang.id)},
    lcid: ${lang.lcid === undefined ? 'None' : `Some(${lang.lcid})`},
    name: ${q(lang.name)},
    functions: &[
${fns.map(([k, v]) => `        (${q(k)}, ${q(v)}),`).join('\n')}
    ],
    errors: &[${errs.join(', ')}],
    bools: ${lang.bools ? `Some([${q(lang.bools[0])}, ${q(lang.bools[1])}])` : 'None'},
    r1c1: ${r1 ? `Some(R1c1Syntax { row: ${q(r1.row)}, col: ${q(r1.col)}, open: ${opt(r1.open)}, close: ${opt(r1.close)} })` : 'None'},
    table: TableKeywordsOpt { all: ${opt(tbl.all)}, data: ${opt(tbl.data)}, headers: ${opt(tbl.headers)}, totals: ${opt(tbl.totals)}, this_row: ${opt(tbl.thisRow)} },
    array: ${arr ? `Some(ArraySeparators { col: ${q(arr.col)}, row: ${q(arr.row)}, alt: ${opt(arr.alt)} })` : 'None'},
    format: ${fmt ? `Some(FormatCodesOpt { year: ${opt(fmt.year)}, month: ${opt(fmt.month)}, day: ${opt(fmt.day)}, hour: ${opt(fmt.hour)}, minute: ${opt(fmt.minute)}, second: ${opt(fmt.second)}, general: ${opt(fmt.general)} })` : 'None'},
    regional: ${reg ? `Some(Regional { decimal: ${q(reg.decimal)}, list: ${q(reg.list)} })` : 'None'},
    source: ${q(lang.source)},
    fn_in: &[
${fnIn.map(([k, v]) => `        (${q(k)}, ${q(v)}),`).join('\n')}
    ],
    err_in: &[${d.errIn.map(([k, c]) => `(${q(k)}, ErrorCode::${ERR_VARIANT[c]})`).join(', ')}],
    kw_in: &[${[...d.kwMap].map(([k, v]) => `(${q(k)}, Kw::${KW_VARIANT[v]})`).join(', ')}],
    kw_out: [${KW_KEYS.map((k) => q(d.table[k])).join(', ')}],
};
`;
}

const registered = langs.languages();
const ids = registered.map((l) => l.id);
if (ids.length !== 2 || ids[0] !== 'en-US' || ids[1] !== 'pt-BR') throw new Error(`registered languages changed: ${ids}`);
if (registered[1] !== ptmod.PT_BR_LANG) throw new Error('pt-BR table identity');

const excelLangs = langs.EXCEL_LANGUAGES.map((l) => `    ExcelLanguageInfo { id: ${q(l.id)}, lcid: ${l.lcid}, name: ${q(l.name)}, function_names: FunctionNames::${l.functionNames[0].toUpperCase()}${l.functionNames.slice(1)}, main: ${!!l.main}, measured: ${!!l.measured} },`);

// ------------------------------------------------------------------ implicit classes
const pc = (c) => `Pc::${c}`;
const impl = Object.entries(implicit.IMPLICIT_CLASSES).sort((x, y) => cpCmp(x[0], y[0]));
const implLines = impl.map(([name, info]) => {
  // names outside the catalogue too (EUROCONVERT): reached when a defined name is called with that name
  return `    (${q(name)}, ${info.top}, &[${info.params.map(pc).join(', ')}]),`;
});

const out = `// GENERATED by core/crates/uo-calc/js/gen-i18n-tables.mjs from the frozen TS (i18n/catalog-data.ts, i18n/languages.ts,
// i18n/langs/pt-BR.ts, implicit-classes.ts; Excel ${data.EXCEL_BUILD}) — do not edit by hand.
//! Static data of the formula-language layer (frontier A5): the Excel catalogue, the registered language tables
//! with the lookup maps \`translate.ts\` builds at run time, the Office interface languages and the measured
//! implicit-intersection classes.

use super::{ArraySeparators, CatalogEntry, ExcelLanguageInfo, FormatCodesOpt, FormulaLanguage, FunctionNames, Kw, R1c1Syntax, Regional, TableKeywordsOpt};
use crate::implicit::ParamClass as Pc;
use crate::value::ErrorCode;

/// The Excel build the tables were measured on.
pub const EXCEL_BUILD: &str = ${q(data.EXCEL_BUILD)};

/// The catalogue in the order of the Excel internal number (\`catalog()\`).
#[rustfmt::skip]
pub static CATALOG: [CatalogEntry; ${rows.length}] = [
${catLines.join('\n')}
];

/// Indices of [\`CATALOG\`] sorted by name (code-point order).
#[rustfmt::skip]
pub static CATALOG_BY_NAME: [u16; ${rows.length}] = [${byName.join(', ')}];

/// Names the Excel table refuses in a cell formula (XLM macro and internal functions), sorted.
#[rustfmt::skip]
pub static RESERVED: [&str; ${reserved.length}] = [${reserved.map(q).join(', ')}];

/// Names in the Excel table without effect in this build (\`#NAME?\`), sorted.
#[rustfmt::skip]
pub static UNAVAILABLE: [&str; ${unavailable.length}] = [${unavailable.map(q).join(', ')}];

/// Functions shown as operators on screen (\`SINGLE\` ↔ \`@\`), sorted by name.
pub static OPERATOR_FUNCTIONS: [(&str, &str); ${opFns.length}] = [${opFns.map(([k, v]) => `(${q(k)}, ${q(v)})`).join(', ')}];

/// The Office 365 interface languages (\`EXCEL_LANGUAGES\`, in the TS order).
#[rustfmt::skip]
pub static EXCEL_LANGUAGES: [ExcelLanguageInfo; ${excelLangs.length}] = [
${excelLangs.join('\n')}
];

${langBlock('EN_US_LANG', langs.EN_US_LANG)}
${langBlock('PT_BR_LANG', ptmod.PT_BR_LANG)}
/// The measured parameter classes of legacy formulas (\`IMPLICIT_CLASSES\`): (name, top, params), sorted by name.
#[rustfmt::skip]
pub static IMPLICIT_CLASSES: [(&str, bool, &[Pc]); ${impl.length}] = [
${implLines.join('\n')}
];
`;

const target = join(C.crateDir, 'src', 'i18n', 'tables.rs');
if (a.flag('--check')) {
  if (readFileSync(target, 'utf8').replace(/\r\n/g, '\n') !== out) throw new Error('src/i18n/tables.rs is stale: rerun gen-i18n-tables.mjs');
  console.log('i18n/tables.rs up to date');
} else {
  writeFileSync(target, out);
  console.log(`wrote ${target}: ${rows.length} catalogue entries, ${impl.length} implicit classes, ${Object.keys(ptmod.PT_BR_LANG.functions).length} pt-BR names`);
}
