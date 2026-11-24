// Parity capture group `bank` (plan section 4.3 "Calc"): the bit-exact TS results of the Excel-measured oracle bank
// (sheet/packages/calc/test/oracle/<area>.cases.json, 71 files, 35,299 cases) through the entry point the TS
// oracle test uses (`runCase(case, PT_BR)` of testing.ts: `callFn` over the mock context, or parse + Engine.recalc
// for whole-formula cases), with the Excel answer and the case status.
//
//   node core/crates/uo-calc/js/capture-bank.mjs                       # CI: pt-BR -> tests/captures/bank.capture
//   node core/crates/uo-calc/js/capture-bank.mjs --locales pt-BR,en-US --out core/target/sheet-cap/bank-big.capture
//   options: --only <area>  --ts-root <dir>
//
// Records: key `<case id>` (`@<locale>` suffix for locales other than pt-BR), facets:
//   src     `<area>:<index>` (the case is read from the oracle file in this worktree)
//   in      `len:sha256[0..16]` of JSON.stringify(case) (a changed input is a stale capture, not a parity failure)
//   ts      canonical value text (lib/common.mjs `makeValueText`; `#len:sha` when over 512 units, full text in the
//           `.values.gz` sidecar: `<key>\t<text>` lines) or `throw:<message>` when the TS threw
//   xl      (status other than match) the Excel answer (`v`/`spill` as a value text) or `err:<message>` (the 47 cases Excel could not answer)
//   st      match | diverge | known | xlerr | noxl  (known = listed in <area>.known.json, with its reason digest in
//           `why`; diverge = TS != Excel and not listed: must be 0 on the frozen reference)
process.env.TZ = 'UTC';
const { readFileSync, readdirSync, existsSync, mkdirSync, writeFileSync } = await import('node:fs');
const { join, dirname } = await import('node:path');
const { gzipSync } = await import('node:zlib');
const { CaptureWriter } = await import('../../uo-testkit/js/testkit.mjs');
const C = await import('./lib/common.mjs');

const a = C.args();
const root = C.tsRoot(a);
const { calc, testing } = await C.loadCalc(root);
const vt = C.makeValueText(calc);
const locales = a.opt('--locales', 'pt-BR').split(',');
const LOC = { 'pt-BR': calc.PT_BR, 'en-US': calc.EN_US };
const only = a.opt('--only');
const out = a.opt('--out', join(C.capturesDir, 'bank.capture'));

const read = (f) => JSON.parse(readFileSync(join(C.oracleDir, f), 'utf8'));
const xlText = (r) => {
  if (!r) return 'none';
  if (r.err) return `err:${C.oneLine(r.err)}`;
  const sc = (x) => {
    if (x !== null && typeof x === 'object') return 'e' in x ? `e${x.e}` : `s${JSON.stringify(x.s)}`;
    return vt(x);
  };
  if (r.spill) return `a${r.spill.length}x${r.spill[0]?.length ?? 0}[${r.spill.map((row) => row.map(sc).join(',')).join(';')}]`;
  return sc(r.v ?? null);
};

const w = new CaptureWriter([
  'Calc bank (plan 4.3): TS runCase over the Excel oracle bank, see core/crates/uo-calc/js/capture-bank.mjs and core/docs/modules/uo-calc.md "Captures"',
]);
const files = readdirSync(C.oracleDir).filter((f) => f.endsWith('.cases.json')).sort();
const bankDigest = C.sha(files.map((f) => `${f}\n${readFileSync(join(C.oracleDir, f), 'utf8')}`).join('\n')).slice(0, 32);
C.stamp(w, root, { locales: locales.join(','), 'oracle-files': files.length, 'oracle-digest': bankDigest });
const side = [];
const st = { cases: 0, match: 0, diverge: 0, known: 0, xlerr: 0, noxl: 0, throw: 0 };
const diverged = [];
const t0 = performance.now();
for (const f of files) {
  const area = f.replace('.cases.json', '');
  if (only && area !== only) continue;
  const cases = read(f);
  const xl = existsSync(join(C.oracleDir, `${area}.excel.json`)) ? new Map(read(`${area}.excel.json`).map((r) => [r.id, r])) : new Map();
  const known = existsSync(join(C.oracleDir, `${area}.known.json`)) ? read(`${area}.known.json`) : {};
  cases.forEach((c, i) => {
    for (const loc of locales) {
      const key = loc === 'pt-BR' ? c.id : `${c.id}@${loc}`;
      w.set(key, 'src', `${area}:${i}`);
      w.set(key, 'in', C.digest(JSON.stringify(c)).slice(0, -16));
      let ours, text;
      try {
        ours = testing.runCase(c, LOC[loc]);
        text = vt(ours);
      } catch (e) {
        text = `throw:${C.oneLine(e?.message ?? e)}`;
        st.throw++;
      }
      const clipped = C.clip(text);
      if (clipped !== text) side.push(`${key}\t${text}`);
      w.set(key, 'ts', clipped);
      const r = xl.get(c.id);
      if (loc !== 'pt-BR') continue; // the oracle is pt-BR Excel: status only for it
      st.cases++;
      let s;
      if (!r) s = 'noxl';
      else if (r.err) s = 'xlerr';
      else if (known[c.id]) s = 'known';
      else s = ours !== undefined && testing.matchesOracle(ours, r) ? 'match' : 'diverge';
      st[s]++;
      w.set(key, 'st', s);
      if (s !== 'match') w.set(key, 'xl', C.clip(xlText(r)));
      if (s === 'known') w.set(key, 'why', C.digest(known[c.id]));
      if (s === 'diverge') diverged.push(c.id);
    }
  });
}
// the locale-file known divergences (i18n tables, not cases): listed for the 777 total
let localeKnown = 0;
for (const f of readdirSync(C.oracleDir).filter((x) => /^locale-.*\.known\.json$/.test(x))) localeKnown += Object.keys(read(f)).filter((k) => !k.startsWith('build')).length;
w.meta('stats', Object.entries(st).map(([k, v]) => `${k}=${v}`).join(' '));
w.meta('locale-known', `${localeKnown} (i18n tables, not bank cases: replayed by the calc-tests group, i18n.test.ts)`);
mkdirSync(dirname(out), { recursive: true });
w.write(out);
const sidePath = out.replace(/\.capture$/, '.values.gz');
writeFileSync(sidePath, gzipSync(Buffer.from(side.length ? side.join('\n') + '\n' : '', 'utf8'), { level: 9 }));
console.log(`bank: ${st.cases} cases (${locales.join(',')}) in ${((performance.now() - t0) / 1000).toFixed(1)} s -> ${out}`);
console.log(`  ${Object.entries(st).map(([k, v]) => `${k} ${v}`).join(', ')}; locale-file known ${localeKnown}; long values ${side.length}`);
if (diverged.length) console.log(`  DIVERGE (not in known.json): ${diverged.slice(0, 20).join(', ')}`);
