// A frente A2 da leva 7 do engine="rust" (registro core/docs/modules/uo-bindings.md "Wave 7 — A2: lists, paste, tables,
// objects and the page"), no DOM falso: a definição de pronto da frente. Os nomes da tabela existem e não colidem com as
// outras frentes; há um handler por nome e nenhum lança sem sessão; os comandos das barras da frente só chegam a métodos
// roteados ou da casca (o `run` por um editor Proxy, como engine-objects.test.ts); os métodos das famílias que as barras
// chamam (chrome-*.ts) estão roteados; e os 51 públicos da frente estão roteados, menos os bloqueios declarados no registro.
import { describe, expect, it } from 'vitest';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { PUBLIC_METHODS } from './engine-public.js';
import { ROUTES_COMPLETION } from './engine-routes-completion.js';
import { ROUTES_FORMAT } from './engine-routes-format.js';
import { ROUTES_OBJECTS } from './engine-routes-objects.js';
import { ROUTES_REVIEW } from './engine-routes-review.js';
import { ROUTES_STORIES } from './engine-routes-stories.js';
import { ROUTED_COMMAND_IDS, ROUTED_PUBLIC, SHELL } from './engine-routes.js';

/** The 51 public methods of A2's families that were disabled at 8bf0d483 (the plan's inventory). */
const A2_METHODS = [
  // lists
  'applyListLibrary', 'applyMultilevelLibrary', 'applyListLevels', 'applyListStyle', 'createListStyle', 'modifyListStyle', 'defineListLevel',
  'defineMultilevelList', 'setListLevel', 'restartNumbering', 'continueNumbering', 'setNumberingValue', 'listInfo', 'documentLists',
  'listStylesInDocument', 'numberingValueInfo', 'listLibrary', 'markerPreview',
  // paste
  'pasteCells', 'hasCellsClip', 'pasteData', 'changeImageFromClipboard',
  // tables
  'insertQuickTable', 'quickTables', 'insertSpreadsheet', 'canEmbedSpreadsheet', 'editEmbeddedObject', 'setTableTool', 'tableStyleList',
  'tableStyleXml', 'tableStyleBases', 'newTableStyleId', 'renderTableStyleXmlThumb', 'saveTableStyle',
  // objects and the page
  'startEditWrapPoints', 'endEditWrapPoints', 'editingWrapPoints', 'startTransparentColorPick', 'editSelectedTextbox', 'textboxTemplates',
  'saveSelectionToTextboxGallery', 'removeTextboxGalleryItem', 'paintWatermarkTemplate', 'paintCoverTemplate', 'paintTocTemplate',
  'ensureBuiltinStyle', 'setSelectObjectsMode', 'isSelectObjectsMode', 'pageFillOptions', 'shapeGradientVariations', 'resolveDmlColor',
];

/** What A2 leaves to a later step, each with its blocker in the record (the list must match exactly). */
const BLOCKED = new Set<string>([]);
/** The names of A2's inventory that frontier A1 routes in wave 7 (`ensureBuiltinStyle`). */
const OTHER_FRONT = new Set(A2_METHODS.filter((n) => ROUTES_FORMAT.public.includes(n)));

/** The 38 bar command ids of the plan; the ones blocked with the previews (decision in the record) are listed apart. */
const A2_COMMANDS = [
  'list:defineBullet', 'list:defineNumber', 'list:setValue', 'list:defineMultilevel', 'list:defineStyle', 'list:indents', 'list:restart', 'list:continue',
  'paste:keep', 'paste:merge', 'paste:picture', 'paste:text', 'paste:cells:overwrite', 'paste:cells:rows', 'paste:cells:nest', 'paste:cells:text',
  'table:excel', 'table:style:new', 'table:style:modify', 'table:painter', 'table:draw', 'table:eraser', 'dlg:pageBorders',
  'pic:wrap:editPoints', 'pic:transparentColor', 'pic:change:clip', 'shp:editText', 'select:objects',
  ...Array.from({ length: 10 }, (_, i) => `toc:level${i}`),
];
const BLOCKED_COMMANDS = new Set<string>([]);

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

describe('engine="rust", frente A2 (leva 7): a tabela de rotas', () => {
  it('os nomes públicos existem no elemento e não colidem com a casca nem com as outras frentes', () => {
    for (const n of ROUTES_COMPLETION.public) {
      expect(PUBLIC_METHODS.includes(n), n).toBe(true);
      expect(SHELL.includes(n), n).toBe(false);
      expect(ROUTES_STORIES.public.includes(n) || ROUTES_OBJECTS.public.includes(n) || ROUTES_REVIEW.public.includes(n) || ROUTES_FORMAT.public.includes(n), n).toBe(false);
      expect(ROUTED_PUBLIC.filter((x) => x === n).length, n).toBe(1);
    }
    expect(new Set(ROUTES_COMPLETION.public).size).toBe(ROUTES_COMPLETION.public.length);
  });

  it('um handler para cada nome, e nenhum lança sem sessão', { timeout: 60_000 }, async () => {
    const { completionHandlers } = await import('./rust-engine-completion.js');
    const fake = { el: { canvases: [], selection: { anchor: { para: 0, offset: 0 }, focus: { para: 0, offset: 0 } }, style: {}, classList: { add() {}, remove() {} } } };
    const h = completionHandlers(fake as never) as Record<string, (...a: unknown[]) => unknown>;
    expect(Object.keys(h).sort()).toEqual([...ROUTES_COMPLETION.public].sort());
    for (const n of ROUTES_COMPLETION.public) {
      const args: unknown[] = n === 'setNumberingValue' ? [{ mode: 'new' }] : n === 'defineListLevel' ? [{ bullet: '•' }] : n === 'pasteData' ? [{ text: 'x' }] : [];
      await expect(Promise.resolve().then(() => h[n]!(...args)), n).resolves.not.toThrow();
    }
  });

  it('os 51 públicos da frente estão roteados (menos os bloqueios declarados e o de A1)', () => {
    const routed = new Set([...ROUTED_PUBLIC, ...SHELL]);
    const left = A2_METHODS.filter((n) => !routed.has(n) && !OTHER_FRONT.has(n));
    // a blocked name frontier A1 routes meanwhile (its own paint of the TOC gallery) leaves the list
    expect(new Set(left)).toEqual(new Set([...BLOCKED].filter((n) => !OTHER_FRONT.has(n))));
    expect(A2_METHODS.length).toBe(51);
    console.log(`[engine-completion] routed by A1: ${[...OTHER_FRONT].join(', ')}`);
    console.log(`[engine-completion] A2: ${A2_METHODS.length - left.length - OTHER_FRONT.size} of ${A2_METHODS.length} routed, blocked: ${[...BLOCKED].join(', ')}`);
  });

  it('os comandos das barras da frente estão em ROUTED_COMMAND_IDS e o run só chega a métodos roteados ou da casca', { timeout: 60_000 }, async () => {
    const cmds = new Map<string, { run: () => unknown }>();
    const reg = { add: (...cs: { id: string; run: () => unknown }[]) => { for (const c of cs) cmds.set(c.id, c); }, get: (id: string) => cmds.get(id) };
    const log: string[] = [];
    const ed = universal(log, 'ed.');
    const app = universal(log, 'app.');
    const ctl = universal(log, 'ctl.');
    const { registerWriteCommands } = await import('./chrome-commands.js');
    registerWriteCommands(reg as never, ed as never, app as never, ctl as never);
    const allowed = new Set([...ROUTED_PUBLIC, ...SHELL]);
    // os diálogos que os comandos abrem (o OK de cada um é dirigido no harness editor_switch_completion.mjs)
    const dialogs = new Set(['ctl.listDialog', 'ctl.tableDialog', 'ctl.clipboard', 'ctl.flash', 'ctl.state', 'ctl.tocDialog']);
    const bad: string[] = [];
    for (const id of A2_COMMANDS) {
      const c = cmds.get(id);
      expect(c, id).toBeTruthy();
      if (BLOCKED_COMMANDS.has(id)) { expect(ROUTED_COMMAND_IDS.has(id), id).toBe(false); continue; }
      expect(ROUTED_COMMAND_IDS.has(id), id).toBe(true);
      log.length = 0;
      try { const r = c!.run(); void Promise.resolve(r).catch(() => undefined); } catch { /* o que foi chamado até ali conta */ }
      for (const call of log) {
        if (call.startsWith('ed.') && !allowed.has(call.slice(3))) bad.push(`${id}: ${call}`);
        if (call.startsWith('ctl.') && !dialogs.has(call)) bad.push(`${id}: ${call}`);
      }
    }
    expect(bad).toEqual([]);
  });

  it('as chamadas `ed.X(` das barras das famílias da frente chegam a métodos roteados ou da casca', () => {
    const files = readdirSync(__dirname).filter((f) => /^chrome-(lists|table|table-style|shape|picture.*|page|toc|clipboard)\.ts$/.test(f));
    const allowed = new Set([...ROUTED_PUBLIC, ...SHELL]);
    const bad = new Set<string>();
    for (const f of files) {
      const src = readFileSync(join(__dirname, f), 'utf8');
      for (const m of src.matchAll(/\bed\.([A-Za-z]\w*)\(/g)) {
        const n = m[1]!;
        if (!A2_METHODS.includes(n)) continue;
        if (!allowed.has(n) && !BLOCKED.has(n) && !OTHER_FRONT.has(n)) bad.add(`${f}: ${n}`);
      }
    }
    expect([...bad]).toEqual([]);
  });
});
