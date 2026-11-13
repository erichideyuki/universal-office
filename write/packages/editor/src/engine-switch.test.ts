// A troca de motor do <universal-write> (engine="rust"; registro core/docs/modules/uo-bindings.md "Milestone 2: the editor
// switch") no DOM falso: sem o atributo nada do motor Rust é importado nem baixado; todo método público está classificado
// (ROUTED, SHELL ou desligado por padrão); com um RustEditor simulado, um método desligado não toca o modelo TS e responde o
// valor neutro, e os roteados chegam ao motor. A parte de verdade (o .wasm, as páginas, o teclado) é o harness Chromium
// core/crates/uo-wasm/tests/browser/editor_switch.mjs.
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { installFakeDom, type FakeDom, type FakeNode } from '../../../test-support/fake-dom.js';
import type { UniversalWriteElement as Editor } from './universal-write.js';
import type { UniversalWriteFontAsset } from './plugins.js';
import { PUBLIC_METHODS } from './engine-public.js';
import { NEUTRAL_QUERIES, ROUTED_COMMAND_IDS, ROUTED_PRIVATE, ROUTED_PUBLIC, SHELL, isMutator } from './engine-routes.js';

const FONTS = join(__dirname, '..', 'demo', 'fonts');
const VARIANTS = { regular: 'regular', bold: 'bold', italic: 'italic', boldItalic: 'bolditalic' } as const;
const FONT_ASSETS: UniversalWriteFontAsset[] = Object.entries(VARIANTS).map(([variant, file]) => ({
  family: 'Liberation Sans', variant: variant as UniversalWriteFontAsset['variant'],
  data: new Uint8Array(readFileSync(join(FONTS, `liberationsans-${file}.ttf`))),
}));

/** o motor simulado: conta a importação e registra as chamadas que chegam a ele */
const probe = { imported: 0, calls: [] as string[] };
vi.mock('./rust-engine.js', async () => {
  probe.imported++;
  const real = await vi.importActual<typeof import('./rust-engine.js')>('./rust-engine.js');
  return {
    ...real,
    engineStats: () => ({ fake: true }),
    startRustEditor: async (el: HTMLElement, proto: object) => {
      const fake: Record<string, unknown> = { warnDisabled: (n: string) => probe.calls.push(`disabled:${n}`), park: () => {}, unpark: () => {} };
      for (const n of [...ROUTED_PUBLIC, ...ROUTED_PRIVATE]) fake[n] = (...a: unknown[]) => { probe.calls.push(n); void a; return undefined; };
      const counts = real.installRoutes(el, fake as unknown as InstanceType<typeof real.RustEditor>, proto);
      (globalThis as Record<string, unknown>)['__routeCounts'] = counts;
      return fake;
    },
  };
});

let dom: FakeDom;
let Ctor: typeof Editor;
const fetched: string[] = [];

beforeAll(async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] });
  dom = installFakeDom();
  vi.stubGlobal('fetch', (u: unknown) => { fetched.push(String(u)); return Promise.reject(new Error('offline')); });
  Ctor = (await import('./universal-write.js')).UniversalWriteElement;
}, 60_000);
afterAll(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
afterEach(() => vi.clearAllTimers());

async function make(engine?: string): Promise<Editor> {
  const ed = new Ctor();
  ed.setAttribute('manual-init', '');
  if (engine !== undefined) ed.setAttribute('engine', engine);
  dom.document.body.appendChild(ed as unknown as FakeNode);
  await ed.initialize({ fonts: FONT_ASSETS });
  return ed;
}

describe('engine="rust": a troca', () => {
  it('sem o atributo (e com outro valor) nada do motor Rust é importado nem baixado', async () => {
    for (const v of [undefined, 'ts', 'Rusty']) {
      const ed = await make(v);
      expect(ed.engine).toBe('ts');
      expect((ed as unknown as { rust: unknown }).rust).toBeNull();
      expect(Ctor.engineStats()).toBeNull();
      (ed as unknown as FakeNode).remove();
    }
    expect(probe.imported).toBe(0);
    expect(fetched.filter((u) => u.includes('.wasm'))).toEqual([]);
  });

  it('todo método público está classificado (a lista gerada bate com o fonte)', () => {
    const src = readFileSync(join(__dirname, 'universal-write.ts'), 'utf8').split('\n');
    const start = src.findIndex((l) => l.startsWith('export class UniversalWriteElement'));
    let end = start + 1;
    while (end < src.length && src[end] !== '}') end++;
    const names = new Set<string>();
    for (const l of src.slice(start, end)) {
      const m = /^ {2}(?:async )?([A-Za-z_][A-Za-z0-9_]*)(?:<[^>]*>)?\(/.exec(l);
      if (m && m[1] !== 'constructor') names.add(m[1]!);
    }
    expect([...names].sort()).toEqual([...PUBLIC_METHODS].sort());
    const proto = Ctor.prototype as unknown as Record<string, unknown>;
    // (wave 5, front O: two public methods indented with 4 spaces in the source are not in engine-public.ts; routed, ED-212)
    const fourSpace = new Set(['setImageLockAnchor', 'setImageEffect'].filter((n) => src.some((l) => l.startsWith(`    ${n}(`))));
    for (const n of ROUTED_PUBLIC) expect(names.has(n) || fourSpace.has(n), `ROUTED ${n}`).toBe(true);
    for (const n of ROUTED_PRIVATE) expect(typeof proto[n], `private ${n}`).toBe('function');
    for (const n of SHELL) if (n !== 'constructor') expect(names.has(n), `SHELL ${n}`).toBe(true);
    for (const n of NEUTRAL_QUERIES) expect(names.has(n) && !ROUTED_PUBLIC.includes(n) && !SHELL.includes(n) && !isMutator(n), `NEUTRAL ${n}`).toBe(true);
    const routed = new Set(ROUTED_PUBLIC), shell = new Set(SHELL);
    const rest = PUBLIC_METHODS.filter((n) => !routed.has(n) && !shell.has(n));
    const mutators = rest.filter(isMutator).length;
    // (wave 7c, S: no name twice — a front's list and the base list held 5 in common; the counts are distinct names over
    // PUBLIC_METHODS, as the merge-state probe counts them)
    expect(new Set(ROUTED_PUBLIC).size, 'ROUTED_PUBLIC without duplicates').toBe(ROUTED_PUBLIC.length);
    expect(new Set(ROUTED_PRIVATE).size, 'ROUTED_PRIVATE without duplicates').toBe(ROUTED_PRIVATE.length);
    const routedPub = PUBLIC_METHODS.filter((n) => routed.has(n)).length;
    const shellPub = PUBLIC_METHODS.filter((n) => shell.has(n)).length;
    const both = PUBLIC_METHODS.filter((n) => routed.has(n) && shell.has(n)).length;
    const indented = ROUTED_PUBLIC.filter((n) => !PUBLIC_METHODS.includes(n)).length;
    console.log(`public methods ${PUBLIC_METHODS.length}: routed ${routedPub} (+ ${indented} indented), shell ${shellPub}, of which both ${both}, disabled mutators ${mutators}, disabled queries ${rest.length - mutators}; private routed ${ROUTED_PRIVATE.length}; bar commands routed ${ROUTED_COMMAND_IDS.size}`);
  });

  it('nenhuma consulta desligada (que roda o código TS) é um mutador disfarçado', () => {
    // a classificação é pelo nome: um mutador cujo nome não casa com MUTATOR viraria "consulta" e rodaria o corpo TS
    // (inerte só porque o modelo TS é null — a rede de segurança, não a regra). Corpo com passo de desfazer = mutador.
    const src = readFileSync(join(__dirname, 'universal-write.ts'), 'utf8').split('\n').map((l) => l.replace(/\r$/, ''));
    const routed = new Set(ROUTED_PUBLIC), shell = new Set(SHELL), neutral = new Set(NEUTRAL_QUERIES);
    const queries = PUBLIC_METHODS.filter((n) => !routed.has(n) && !shell.has(n) && !neutral.has(n) && !isMutator(n));
    const bad: string[] = [];
    for (const n of queries) {
      const head = new RegExp(`^ {2}(?:async )?${n}(?:<[^>]*>)?\\(`);
      for (let i = 0; i < src.length; i++) {
        if (!head.test(src[i]!)) continue;
        let j = i;
        // corpo de uma linha (`getX(): T { return …; }`): só ela; senão até o `}` da indentação do método
        if (!/\}\s*$/.test(src[i]!)) while (j < src.length && src[j] !== '  }') j++;
        if (src.slice(i, j).some((l) => /this\.(snapshot|applyPara|applyRun|pushHistory)\(/.test(l))) bad.push(n);
        i = j;
      }
    }
    expect(bad).toEqual([]);
  });

  it('com o motor (simulado): roteados chegam ao motor, desligados não tocam o modelo TS e respondem o neutro', async () => {
    const ed = await make('rust');
    expect(probe.imported).toBe(1);
    expect(ed.engine).toBe('rust');
    expect(Ctor.engineStats()).toEqual({ fake: true });
    const counts = (globalThis as Record<string, unknown>)['__routeCounts'] as Record<string, number>;
    expect(counts['disabledMutators']! + counts['disabledQueries']! + ROUTED_PUBLIC.length + SHELL.length - 1).toBeGreaterThanOrEqual(PUBLIC_METHODS.length - 1);
    // o initialize() abriu o documento novo pelo motor (newDocument roteado), não pelo TS
    expect(probe.calls).toContain('newDocument');
    const model = (): unknown => (ed as unknown as { model: unknown }).model;
    expect(model()).toBeNull();
    probe.calls.length = 0;
    ed.toggleRunProp('bold');
    ed.undo();
    expect(probe.calls).toEqual(['toggleRunProp', 'undo']);
    // mutadores desligados: no-op avisado, o modelo TS continua vazio
    // wave 7 (A2, decision 7 of the record): every public method is routed now (Draw Table, Edit Wrap Points…), so the
    // fail-closed path is probed with SYNTHETIC public mutators no routing table classifies (one sync, one async): a second
    // element started with them on the prototype answers a warned no-op and the TS model stays untouched
    const routedNow = new Set([...ROUTED_PUBLIC, ...SHELL]);
    expect(PUBLIC_METHODS.filter((n) => !routedNow.has(n) && isMutator(n))).toEqual([]);
    const proto = Ctor.prototype as unknown as Record<string, unknown>;
    const list = PUBLIC_METHODS as string[];
    const synth = ['setUnclassifiedProbe', 'insertUnclassifiedProbeAsync'];
    let tsRan = 0;
    proto[synth[0]!] = function (this: unknown) { tsRan++; };
    proto[synth[1]!] = async function (this: unknown) { tsRan++; };
    list.push(...synth);
    try {
      expect(synth.every((n) => isMutator(n) && !routedNow.has(n))).toBe(true);
      const ed2 = await make('rust');
      const counts2 = (globalThis as Record<string, unknown>)['__routeCounts'] as Record<string, number>;
      expect(counts2['disabledMutators']).toBe(2);
      probe.calls.length = 0;
      const call = (n: string): unknown => (ed2 as unknown as Record<string, () => unknown>)[n]!();
      expect(call(synth[0]!)).toBeUndefined();
      const r = call(synth[1]!);
      expect(r).toBeInstanceOf(Promise);
      expect(await r).toBeUndefined();
      expect(probe.calls).toEqual(synth.map((n) => `disabled:${n}`));
      expect(tsRan).toBe(0);
      expect((ed2 as unknown as { model: unknown }).model).toBeNull();
      (ed2 as unknown as FakeNode).remove();
    } finally {
      for (const n of synth) delete proto[n];
      list.splice(list.length - synth.length, synth.length);
    }
    probe.calls.length = 0;
    ed.toggleRunProp('bold');
    expect(probe.calls).toEqual(['toggleRunProp']);
    expect(model()).toBeNull();
    // consultas desligadas: o valor do TS com o modelo vazio
    expect(ed.getComments()).toEqual([]);
    expect(ed.getRevisions()).toEqual([]);
    expect(ed.editingHeaderFooter()).toBeNull();
    expect(ed.editingTextbox()).toBe(false);
    // consultas que o modelo TS vazio responderia com um valor falso do documento: o neutro direto (ED-24)
    expect(ed.listStyles('gallery')).toEqual([]);
    expect(ed.paragraphStyles()).toEqual([]);
    expect(ed.listStylesInDocument()).toEqual([]);
    expect(ed.isTrackingChanges()).toBe(false);
    // as opções de fontes no arquivo e os estilos do cursor são do documento: roteadas
    probe.calls.length = 0;
    ed.getFontEmbedding();
    ed.setFontEmbedding({ embed: true });
    ed.currentStyles();
    expect(probe.calls).toEqual(['getFontEmbedding', 'setFontEmbedding', 'currentStyles']);
    (ed as unknown as FakeNode).remove();
  });
});
