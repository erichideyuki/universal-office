// A frente O da leva 5 do engine="rust" (registro core/docs/modules/uo-bindings.md "Wave 5 — O: objects and the page"), no
// DOM falso: a tabela de rotas da frente está classificada (nomes públicos que existem, sem colisão com a casca ou com as
// outras frentes), os handlers existem e não lançam sem sessão, e cada comando das barras que a frente liga
// (ROUTES_OBJECTS.commands) só chega a métodos roteados ou da casca no `run` (um editor Proxy registra as chamadas). Mais a
// aritmética do fim de arrasto (o `posFor` do TS × o `LayoutPos` da sessão) e as conversões TS → fio.
import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { installFakeDom } from '../../../test-support/fake-dom.js';
import { PUBLIC_METHODS } from './engine-public.js';
import { ROUTES_OBJECTS } from './engine-routes-objects.js';
import { ROUTES_REVIEW } from './engine-routes-review.js';
import { ROUTES_STORIES } from './engine-routes-stories.js';
import { ROUTED_PUBLIC, SHELL } from './engine-routes.js';

/** Os públicos recuados com 4 espaços no fonte (fora de engine-public.ts; ED-212). */
const FOUR_SPACE = ['setImageLockAnchor', 'setImageEffect'];

/** Um valor universal: chamável, toda propriedade é ele mesmo, vira 0/'' em contas. */
function universal(log: string[], prefix: string): unknown {
  const fn = function () { /* */ };
  const p: unknown = new Proxy(fn, {
    get(_t, k) {
      if (k === Symbol.toPrimitive) return () => 0;
      if (k === 'then') return undefined;
      if (typeof k === 'symbol') return undefined;
      if (k === 'length') return 0;
      return new Proxy(fn, {
        apply() { log.push(`${prefix}${String(k)}`); return p; },
        get(_t2, k2) { if (k2 === 'apply' || k2 === 'call') return () => { log.push(`${prefix}${String(k)}`); return p; }; return (p as Record<string | symbol, unknown>)[k2]; },
      });
    },
    apply() { return p; },
  });
  return p;
}

describe('engine="rust", frente O: a tabela de rotas', () => {
  it('os nomes públicos existem no elemento (ou são os recuados com 4 espaços) e não colidem', () => {
    const src = readFileSync(join(__dirname, 'universal-write.ts'), 'utf8');
    for (const n of ROUTES_OBJECTS.public) {
      const known = PUBLIC_METHODS.includes(n) || (FOUR_SPACE.includes(n) && new RegExp(`^ {4}${n}\\(`, 'm').test(src));
      expect(known, n).toBe(true);
      expect(SHELL.includes(n), n).toBe(false);
      expect(ROUTES_STORIES.public.includes(n) || ROUTES_REVIEW.public.includes(n), n).toBe(false);
      expect(ROUTED_PUBLIC.filter((x) => x === n).length, n).toBe(1);
    }
    expect(new Set(ROUTES_OBJECTS.public).size).toBe(ROUTES_OBJECTS.public.length);
  });

  it('um handler para cada nome, e nenhum lança sem sessão (o editor simulado do vitest)', { timeout: 60_000 }, async () => {
    const { objectsHandlers } = await import('./rust-engine-objects.js');
    const fake = { el: { canvases: [], selection: { anchor: { para: 0, offset: 0 }, focus: { para: 0, offset: 0 } }, style: {}, classList: { add() {}, remove() {} } } };
    const h = objectsHandlers(fake as never) as Record<string, (...a: unknown[]) => unknown>;
    expect(Object.keys(h).sort()).toEqual([...ROUTES_OBJECTS.public].sort());
    for (const n of ROUTES_OBJECTS.public) {
      const args: unknown[] = n === 'setColumns' ? ['two'] : n === 'setPageSetup' || n === 'setImageLayout' || n === 'setShapeLayout' || n === 'setImageSize' || n === 'setShapeSize'
        || n === 'setImageAltText' || n === 'setShapeAltText' || n === 'setImageFormat' || n === 'setShapeFormat' || n === 'setImageBorder' || n === 'setShapeOutline'
        || n === 'setTextboxOptions' || n === 'setImageAdjust' || n === 'setHyphenationOptions' || n === 'setLineNumberingOptions' ? [{}] : n === 'compressImages' ? [{ ppi: 150, deleteCropped: false }] : [];
      await expect(Promise.resolve().then(() => h[n]!(...args)), n).resolves.not.toThrow();
    }
  });

  it('os comandos das barras da frente só chegam a métodos roteados ou da casca (o run, por um editor Proxy)', { timeout: 60_000 }, async () => {
    const cmds = new Map<string, { run: () => unknown }>();
    const reg = { add: (...cs: { id: string; run: () => unknown }[]) => { for (const c of cs) cmds.set(c.id, c); }, get: (id: string) => cmds.get(id) };
    const log: string[] = [];
    const ed = universal(log, 'ed.');
    const app = universal(log, 'app.');
    const ctl = universal(log, 'ctl.');
    const { registerWriteCommands } = await import('./chrome-commands.js');
    registerWriteCommands(reg as never, ed as never, app as never, ctl as never);
    const allowed = new Set([...ROUTED_PUBLIC, ...SHELL]);
    // os diálogos que os comandos da frente abrem: o OK de cada um lido (registro "Wave 5 — O", a lista dos diálogos)
    const dialogs = new Set(['ctl.pageDialog', 'ctl.columnsDialog', 'ctl.pageBgDialog', 'ctl.textLayoutDialog', 'ctl.pictureDialog', 'ctl.image', 'ctl.state', 'ctl.flash', 'ctl.picturePane', 'ctl.shapePane', 'ctl.selectionPane', 'ctl.altTextPane', 'ctl.lineNumbersDialog']);
    const bad: string[] = [];
    for (const id of ROUTES_OBJECTS.commands) {
      const c = cmds.get(id);
      expect(c, id).toBeTruthy();
      log.length = 0;
      try { const r = c!.run(); void Promise.resolve(r).catch(() => undefined); } catch { /* o valor universal não serve a todo run: o que foi chamado até ali conta */ }
      for (const call of log) {
        if (call.startsWith('ed.') && !allowed.has(call.slice(3))) bad.push(`${id}: ${call}`);
        if (call.startsWith('ctl.') && !dialogs.has(call)) bad.push(`${id}: ${call}`);
      }
    }
    expect(bad).toEqual([]);
  });
});

describe('engine="rust", frente O: aritmética e conversões', () => {
  it('o fim de arrasto pela sessão (LayoutPos) grava o que o posFor do TS grava (o método real; ±1 EMU, ED-202)', { timeout: 60_000 }, async () => {
    // o posFor/refRect PRIVADOS do elemento TS (universal-write.ts), não uma cópia
    installFakeDom();
    const proto = (await import('./universal-write.js')).UniversalWriteElement.prototype as unknown as { posFor(...a: unknown[]): unknown };
    const posFor = (cur: unknown, axis: 'h' | 'v', xy: number, frame: unknown): unknown => proto.posFor.call(proto, cur, axis, xy, frame);
    const { layoutPosFor, refRect } = await import('./rust-objects-pointer.js');
    let seed = 7;
    const rnd = (): number => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648; };
    const froms = ['page', 'margin', 'column', 'leftMargin', 'rightMargin', 'insideMargin', 'outsideMargin', 'paragraph', 'line', 'topMargin', 'bottomMargin'];
    const run = (fractional: boolean): { n: number; differ: number; maxDiff: number } => {
      let n = 0, differ = 0, maxDiff = 0;
      const q = (v: number): number => (fractional ? v : Math.round(v));
      for (let i = 0; i < 4000; i++) {
        // as do layout: retângulos fracionários (h.rect.x + res.dx), referências em twips do layout
        const frame = { pageW: 11906, pageH: 16838, marL: 1701, marR: 1134, marT: 1417, marB: 1417, colX: q(1701 + rnd() * 400), colW: 9000, bodyTop: q(1417 + rnd() * 30), paraTop: q(1417 + rnd() * 12000) };
        const axis = rnd() < 0.5 ? 'h' as const : 'v' as const;
        const cur = { relativeFrom: froms[Math.floor(rnd() * froms.length)]!, ...(rnd() < 0.3 ? { pct: 1000 } : {}), ...(rnd() < 0.2 ? { align: 'center' } : {}) };
        const xy = q(rnd() * 16000 - 500);
        const ts = posFor(cur, axis, xy, frame) as { offset?: number; pct?: number; fallback?: number };
        const wire = layoutPosFor(cur, axis, xy, frame) as { offsetPt?: number; pct?: number };
        // o que a sessão grava (pictures.rs set_image_layout `pos`)
        const ro = refRect(cur.relativeFrom, axis, frame).o;
        const span = axis === 'h' ? frame.pageW : frame.pageH;
        const rs = wire.pct !== undefined
          ? { pct: Math.round(wire.pct * 1000), fallback: Math.round((ro + (wire.pct / 100) * span) * 635) }
          : { offset: Math.round((wire.offsetPt ?? 0) * 12700) };
        n++;
        const d = Math.max(Math.abs((ts.offset ?? 0) - (rs.offset ?? 0)), Math.abs((ts.pct ?? 0) - (rs.pct ?? 0)), Math.abs((ts.fallback ?? 0) - (rs.fallback ?? 0)));
        if (d > 0) differ++;
        maxDiff = Math.max(maxDiff, d);
      }
      return { n, differ, maxDiff };
    };
    // twips inteiros: idênticos; frações do layout: a mesma conta em outra ordem de ponto flutuante, no máximo 1 EMU
    // (ou 1 milésimo de %) num .5 de fronteira (ED-202)
    const ints = run(false);
    expect(ints).toEqual({ n: 4000, differ: 0, maxDiff: 0 });
    const fr = run(true);
    expect(fr.n).toBe(4000);
    expect(fr.maxDiff).toBeLessThanOrEqual(1);
    console.log(`[engine-objects] posFor × LayoutPos, frações: ${fr.differ}/${fr.n} diferem em 1`);
  });

  it('as conversões TS → fio (cores, Nullable, preenchimento) e de volta', async () => {
    const { wireColor, wireFormat, wireShapeFill, fromWire } = await import('./rust-objects-core.js');
    expect(wireColor({ kind: 'schemeClr', val: 'accent1', mods: [['lumMod', 75000]] })).toEqual({ kind: 'schemeClr', val: 'accent1', mods: [{ f0: 'lumMod', f1: 75000 }] });
    expect(wireFormat({ line: null, softEdge: 5, glow: { rad: 1, color: { kind: 'srgbClr', val: 'FF0000' } } })).toEqual({ line: { t: 'null' }, glow: { t: 'val', rad: 1, color: { kind: 'srgbClr', val: 'FF0000' } }, softEdge: { t: 'val', v: 5 } });
    expect(wireShapeFill({ kind: 'solid', color: { kind: 'srgbClr', val: '00FF00' } })).toEqual({ t: 'solid', kind: 'srgbClr', val: '00FF00' });
    expect(fromWire({ line: { t: 'null' }, adj: [{ f0: 'adj', f1: 5 }], polygon: { pts: [{ f0: 1, f1: 2 }] }, dist: { t: 0, b: 0, l: 1, r: 1 } }))
      .toEqual({ line: null, adj: { adj: 5 }, polygon: { pts: [[1, 2]] }, dist: { t: 0, b: 0, l: 1, r: 1 } });
  });
});
