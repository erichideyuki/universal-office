// Module-loader hook of the P3 facets (registered by facets.mjs before the frozen engine is
// imported): rewrites, in memory only, the exported functions named in the configuration so
// every call — from other modules and from inside its own module — goes through a wrapper that
// reports `(name, args, result)` to `globalThis.__uoFacet`. The dist files on disk are never
// touched; with no reporter installed the wrapper is a plain pass-through.
//
//   export function NAME(   ->   function NAME__uo(   +   export function NAME(...a) { … }
let wraps = new Map(); // absolute file URL -> [function names]

export async function initialize(data) {
  wraps = new Map(Object.entries(data?.wraps ?? {}));
}

export async function load(url, context, nextLoad) {
  const out = await nextLoad(url, context);
  const names = wraps.get(url);
  if (!names || out.format !== 'module') return out;
  let src = typeof out.source === 'string' ? out.source : Buffer.from(out.source).toString('utf8');
  for (const name of names) {
    const decl = `export function ${name}(`;
    if (!src.includes(decl)) throw new Error(`facet hook: ${url} has no "${decl}"`);
    src = src.replace(decl, `function ${name}__uo(`);
    src += `\nexport function ${name}(...a) { const r = ${name}__uo(...a); const f = globalThis.__uoFacet; if (f) f(${JSON.stringify(name)}, a, r); return r; }\n`;
  }
  return { ...out, source: src, shortCircuit: true };
}
