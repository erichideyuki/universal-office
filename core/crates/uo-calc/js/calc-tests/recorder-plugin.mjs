// Vite plugin of the `calc-tests` capture: rewrites, in memory, the modules of the frozen TS that are entry points
// (never the files on disk):
//   * every `export function NAME(` of calc/src/**  ->  a wrapper NAME that calls globalThis.__uoRec.call(...)
//     around the original body (renamed __uo_NAME; both are hoisted function declarations, so module cycles and
//     call order are unchanged);
//   * the exported classes Engine (calc/src/engine.ts), SheetCalc and XfPool (sheet/src/sheet-calc.ts): the export
//     binding becomes a Proxy that records `new X(...)`, and their PUBLIC methods (from the dist .d.ts) are wrapped
//     on the prototype.
// The recorder keeps only the OUTERMOST call (depth 0): the calls the test itself made.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const norm = (p) => p.replace(/\\/g, '/');

/** Public method names of `export declare class NAME` in a .d.ts (lines not marked private/protected/static). */
function publicMethods(dts, name) {
  const text = readFileSync(dts, 'utf8');
  const start = text.indexOf(`export declare class ${name}`);
  if (start < 0) return [];
  const body = text.slice(text.indexOf('{', start) + 1);
  let depth = 1, end = 0;
  for (; end < body.length && depth > 0; end++) { if (body[end] === '{') depth++; else if (body[end] === '}') depth--; }
  const out = new Set();
  for (const line of body.slice(0, end).split('\n')) {
    if (line.match(/^\s{4}\S/) === null) continue; // members are indented 4 spaces
    const m = /^\s{4}(?:readonly\s+)?(get\s+|set\s+)?([A-Za-z_$][\w$]*)\s*[(<]/.exec(line);
    if (!m || m[1] || /^\s{4}(private|protected|static|constructor)\b/.test(line)) continue;
    out.add(m[2]);
  }
  out.delete('constructor');
  return [...out];
}

export function recorderPlugin({ packages }) {
  const calcSrc = norm(join(packages, 'calc/src/'));
  const sheetCalc = norm(join(packages, 'sheet/src/sheet-calc.ts'));
  const classes = {
    [norm(join(packages, 'calc/src/engine.ts'))]: [['Engine', publicMethods(join(packages, 'calc/dist/engine.d.ts'), 'Engine')]],
    [sheetCalc]: [
      ['SheetCalc', publicMethods(join(packages, 'sheet/dist/sheet-calc.d.ts'), 'SheetCalc')],
      ['XfPool', publicMethods(join(packages, 'sheet/dist/sheet-calc.d.ts'), 'XfPool')],
    ],
  };
  return {
    name: 'uo-calc-recorder',
    transform(code, id) {
      const file = norm(id.split('?')[0]);
      const inCalc = file.startsWith(calcSrc) && file.endsWith('.ts');
      if (!inCalc && file !== sheetCalc) return null;
      const mod = file.startsWith(calcSrc) ? `calc/${file.slice(calcSrc.length).replace(/\.ts$/, '')}` : 'sheet/sheet-calc';
      let out = code;
      if (inCalc) {
        const names = [...code.matchAll(/^export function ([A-Za-z_$][\w$]*)\s*\(/gm)].map((m) => m[1]);
        const once = names.filter((n, i) => names.indexOf(n) === i && names.lastIndexOf(n) === i);
        for (const n of once) {
          out = out.replace(new RegExp(`^export function ${n.replace(/\$/g, '\\$')}\\s*\\(`, 'm'), () =>
            `export function ${n}(...__a) { const r = globalThis.__uoRec; return r !== undefined && r.depth === 0 ? r.call(${JSON.stringify(`${mod}:${n}`)}, __uo_${n}, this, __a) : __uo_${n}.apply(this, __a); }\nfunction __uo_${n}(`);
        }
      }
      for (const [cls, methods] of classes[file] ?? []) {
        const re = new RegExp(`^export class ${cls}\\b`, 'm');
        if (!re.test(out)) continue;
        out = out.replace(re, `class ${cls}`);
        out += `\nconst __uo_cls_${cls} = globalThis.__uoRec ? globalThis.__uoRec.wrapClass(${cls}, ${JSON.stringify(`${mod}:${cls}`)}, ${JSON.stringify(methods)}) : ${cls};\nexport { __uo_cls_${cls} as ${cls} };\n`;
      }
      return out === code ? null : { code: out, map: null };
    },
  };
}
