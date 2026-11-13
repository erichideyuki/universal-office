/**
 * Wave 7 (B, gate-O5): the Rust engine's font supply (`RustSession.relayout`). uo-fonts reports each missing (family,
 * variant) ONCE per environment (`take_missing`), so the loop must (a) fetch the faces a first push still lacks — the
 * resolver had only the regular face, the engine asked for bold — and (b) keep a list taken outside the loop (the editor's
 * synchronous relayout) for the next pass. Before the fix the bold of a table cell was laid out with the regular advances
 * (synthetic bold: 'quebra de linhas celulas' fitted in 109.2 pt; Word and the TS break it). No wasm: fake module, env,
 * resolver and document.
 */
import { describe, expect, it } from 'vitest';
import type { FontResolver } from '@universal-office/fonts';
import { RustSession, type EngineDocument } from '../src/rust-engine.js';

type Variant = 'regular' | 'bold' | 'italic' | 'boldItalic';
type Missing = { family: string; variants: Variant[] };

/** the page environment is module-wide (one per fonts base): each test frees the previous one */
let lastEnv: { freed: boolean } | null = null;

function fakes(opts: { faces: Variant[]; reportOnce: Missing[] }) {
  const faces = new Set<Variant>(opts.faces);
  const loads: string[] = [];
  const pushes: string[][] = [];
  let noted: Missing[] = [];
  const attempted = new Set<string>();
  const bytes = (v: Variant): Uint8Array => new Uint8Array(v.length + 10);
  const result = (family: string) => ({
    family, source: 'embedded', embeddable: true, cssFamily: family,
    faces: Object.fromEntries([...faces].map((v) => [v, bytes(v)])) as Partial<Record<Variant, Uint8Array>>,
  });
  const resolver = {
    suggestBaseUrl() {},
    getWithFile: (f: string) => result(f),
    resolveWithFile: async (fs: Iterable<string>) => [...fs].map(result),
    loadFace: async (_r: unknown, v: Variant) => { loads.push(v); faces.add(v); return bytes(v); },
  } as unknown as FontResolver;
  if (lastEnv) lastEnv.freed = true;
  const env = {
    freed: false,
    inner: {},
    pushFamily: (_f: string, o: { faces: { variant: Variant }[] }) => { pushes.push(o.faces.map((x) => x.variant)); },
    pushAsset() {},
    registerHyphenation() {},
    /** uo-fonts take_missing: once per (family, variant) per environment */
    takeMissing: (): Missing[] => {
      const out: Missing[] = [];
      for (const m of noted) {
        const vs = m.variants.filter((v) => { const k = `${m.family}|${v}`; if (attempted.has(k)) return false; attempted.add(k); return true; });
        if (vs.length) out.push({ family: m.family, variants: vs });
      }
      noted = [];
      return out;
    },
  };
  /** what a layout notes: every requested face the environment does not hold yet */
  const note = (): void => {
    const held = new Set(pushes.flat());
    noted = opts.reportOnce.map((m) => ({ family: m.family, variants: m.variants.filter((v) => !held.has(v)) })).filter((m) => m.variants.length);
  };
  lastEnv = env;
  let layouts = 0;
  const dl = () => ({ freed: false, free() { this.freed = true; }, pages: [] });
  const doc: EngineDocument = {
    layout: () => { layouts++; note(); return dl() as never; },
    hyphenationPatterns: () => [],
    free() {},
  };
  const mod = { dead: false, newEnv: () => env, patternText: async () => null, stats: () => null };
  const host = { fontsBase: () => null, resolver, repaint() {}, source: () => undefined };
  const rs = new RustSession(host);
  rs.adoptDocument(mod as never, doc);
  return { rs, env, loads, pushes, note, layouts: () => layouts };
}

describe('wave 7 (B): the Rust engine font loop', () => {
  it('fetches a requested face the first push lacks (resolver had only the regular face)', async () => {
    const f = fakes({ faces: ['regular'], reportOnce: [{ family: 'Carlito', variants: ['regular', 'bold'] }] });
    const seen: number[] = [];
    await f.rs.relayout(() => false, { view: 'print', webWidthTw: 0 }, (_dl, pass) => seen.push(pass));
    expect(f.loads).toEqual(['bold']);
    expect(f.pushes.at(-1)).toEqual(['regular', 'bold']);
    expect(seen.length).toBeGreaterThanOrEqual(2); // a layout after the bold face arrived
  });

  it('a missing list taken outside the loop (carryMissing) reaches the next pass', async () => {
    const f = fakes({ faces: ['regular', 'bold'], reportOnce: [{ family: 'Carlito', variants: ['bold'] }] });
    // the editor's synchronous relayout: lays out, takes the missing list (consumed in the env) and hands it over
    f.note();
    const taken = f.env.takeMissing();
    expect(taken).toEqual([{ family: 'Carlito', variants: ['bold'] }]);
    expect(f.env.takeMissing()).toEqual([]); // reported once
    f.rs.carryMissing(taken);
    await f.rs.relayout(() => false, { view: 'print', webWidthTw: 0 }, () => {});
    expect(f.pushes.at(-1)).toEqual(['regular', 'bold']);
    expect(f.layouts()).toBeGreaterThanOrEqual(2);
  });

  it('without carrying, the list is lost (the defect the editor had)', async () => {
    const f = fakes({ faces: ['regular', 'bold'], reportOnce: [{ family: 'Carlito', variants: ['bold'] }] });
    f.note();
    f.env.takeMissing();
    await f.rs.relayout(() => false, { view: 'print', webWidthTw: 0 }, () => {});
    expect(f.pushes).toEqual([]);
  });
});
