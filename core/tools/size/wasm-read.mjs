// Shared reader of the size tools (core/docs/modules/size-optimization.md): the sections of a wasm module and its
// function bodies with their demangled names. Names need a build that keeps the name section
// (CARGO_PROFILE_WASM_STRIP=false); demangling goes through `wasm-tools demangle` (cargo install wasm-tools).
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';

const SECTION = ['custom', 'type', 'import', 'function', 'table', 'memory', 'global', 'export', 'start', 'element', 'code', 'data', 'datacount'];

/** `{ bytes, sections: [{ label, start, size }], functions: [{ name, body }] }` (`name` demangled, crate hashes removed) */
export function readModule(file, { demangle = true } = {}) {
  const fileSize = readFileSync(file).length;
  let buf = readFileSync(file);
  if (demangle && buf.includes(Buffer.from('name'))) {
    const dir = mkdtempSync(join(tmpdir(), 'uo-size-'));
    try {
      const out = join(dir, 'dm.wasm');
      execFileSync('wasm-tools', ['demangle', file, '-o', out]);
      buf = readFileSync(out);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }
  let p = 8;
  const leb = () => { let r = 0, s = 0, b; do { b = buf[p++]; r += (b & 0x7f) * 2 ** s; s += 7; } while (b & 0x80); return r; };
  // a length-prefixed field: read the length first, then advance (`p += leb()` would read `p` before leb() moves it)
  const skipName = () => { const n = leb(); p += n; };
  const sections = [];
  let codeStart = 0, nameStart = 0, nameEnd = 0, imported = 0;
  while (p < buf.length) {
    const id = buf[p++]; const size = leb(); const start = p;
    let label = SECTION[id] ?? `id${id}`;
    if (id === 0) { const n = leb(); const nm = buf.subarray(p, p + n).toString(); label = `custom:${nm}`; if (nm === 'name') { nameStart = p + n; nameEnd = start + size; } }
    if (id === 2) {
      const n = leb();
      for (let i = 0; i < n; i++) {
        skipName(); skipName();
        const kind = buf[p++];
        if (kind === 0) { leb(); imported++; } else if (kind === 1) { p++; const f = leb(); leb(); if (f & 1) leb(); } else if (kind === 2) { const f = leb(); leb(); if (f & 1) leb(); } else if (kind === 3) p += 2;
      }
    }
    if (id === 10) codeStart = start;
    sections.push({ label, start, size });
    p = start + size;
  }
  const names = new Map();
  if (nameStart) {
    p = nameStart;
    while (p < nameEnd) {
      const sub = buf[p++]; const len = leb(); const end = p + len;
      if (sub === 1) { const n = leb(); for (let i = 0; i < n; i++) { const idx = leb(); const l = leb(); names.set(idx, buf.subarray(p, p + l).toString().replace(/\[[0-9a-f]{16}\]/g, '')); p += l; } }
      p = end;
    }
  }
  const functions = [];
  if (codeStart) {
    p = codeStart; const count = leb();
    for (let i = 0; i < count; i++) { const s = leb(); functions.push({ name: names.get(imported + i) ?? '?', body: buf.subarray(p, p + s) }); p += s; }
  }
  return { fileSize, bytes: buf, sections, functions, hasNames: names.size > 0 };
}

/** the name with every generic argument list collapsed to `<_>` and closure numbers removed */
export function collapseGenerics(n) {
  let out = '', i = 0;
  while (i < n.length) {
    const c = n[i];
    const prev = out[out.length - 1] ?? '';
    if (c === '<' && (/[A-Za-z0-9_\]]/.test(prev) || out.endsWith('::'))) {
      let d = 0, j = i;
      for (; j < n.length; j++) { if (n[j] === '<') d++; else if (n[j] === '>') { d--; if (d === 0) break; } }
      out += '<_>'; i = j + 1; continue;
    }
    out += c; i++;
  }
  return out.replace(/\{closure#\d+\}/g, '{closure}').replace(/::<_>/g, '');
}

/** the crate that owns a function: the type of `<T as Trait>::m` / `<T>::m`, else the path */
export function ownerCrate(n) {
  let s = n;
  if (s.startsWith('<')) {
    let d = 0, i = 0;
    for (; i < s.length; i++) { if (s[i] === '<') d++; else if (s[i] === '>') { d--; if (d === 0) break; } }
    s = s.slice(1, i);
  }
  s = s.replace(/^&(mut )?/, '').replace(/^dyn /, '').replace(/^\[\(?/, '');
  return s.split(/::|[<\s,]/)[0] || '?';
}
