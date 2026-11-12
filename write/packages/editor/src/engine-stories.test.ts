// Frente S da leva 5 do engine="rust" (registro core/docs/modules/uo-bindings.md "Wave 5 — S: stories"): a tabela de rotas
// da frente e os handlers. (a) cada nome público roteado tem handler; (b) cada id de comando roteado das barras só chega
// a métodos roteados ou da casca (leitura estática do `run` em chrome-*.ts); (c) sem sessão (o motor falso) os handlers
// respondem o neutro do TS e não lançam. A parte de verdade é o harness Chromium
// core/crates/uo-wasm/tests/browser/editor_switch_stories.mjs.
import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { ROUTES_STORIES } from './engine-routes-stories.js';
import { ROUTES_OBJECTS } from './engine-routes-objects.js';
import { ROUTES_REVIEW } from './engine-routes-review.js';
import { ROUTED_PUBLIC, SHELL } from './engine-routes.js';
import { PUBLIC_METHODS } from './engine-public.js';
import { storiesHandlers } from './rust-engine-stories.js';
import type { RustEditor } from './rust-engine.js';

/** um RustEditor sem sessão (o do teste de rotas): nenhum `frontApi`, nenhum módulo */
const stub = { el: null } as unknown as RustEditor;

/** chamadas `ed.X(` que um comando roteado pode fazer sem ser roteado (declaradas no registro) */
const ALLOWED = new Set([
  // a paleta de cores do tema: consulta desligada sobre o modelo TS vazio = as cores do tema do Office (ED-105)
  'documentThemeColors',
]);

/** Os ids de chrome-table.ts / chrome-hf.ts que ficam DESLIGADOS com engine="rust" (o resto declarado, D7 do registro). */
const DISABLED_IDS: Record<string, string> = {
  'table:excel': 'insertSpreadsheet/canEmbedSpreadsheet: the embedded spreadsheet (not routed)',
  'table:style:new': 'saveTableStyle: New Table Style (not routed)',
  'table:style:modify': 'saveTableStyle: Modify Table Style (not routed)',
  'table:painter': 'setTableTool: the Border Painter tool (not routed)',
  'table:draw': 'setTableTool: the Draw Table tool (not routed)',
  'table:eraser': 'setTableTool: the Eraser tool (not routed)',
  'paste:cells:overwrite': 'pasteCells/hasCellsClip (not routed)',
  'paste:cells:rows': 'pasteCells/hasCellsClip (not routed)',
  'paste:cells:nest': 'pasteCells/hasCellsClip (not routed)',
  'paste:cells:text': 'pasteCells/hasCellsClip (not routed)',
  'dlg:borders': 'the Borders and Shading dialog: setParagraphBorders (not routed)',
  'dlg:pageBorders': 'setPageBorders: frontier O (page borders)',
};

/** As chamadas `ed.X(` de chrome-table.ts / chrome-hf.ts a métodos não roteados (no-op ou neutro com engine="rust"). */
const DISABLED_CALLS: Record<string, string> = {
  canEmbedSpreadsheet: 'table:excel (disabled id)',
  insertSpreadsheet: 'table:excel (disabled id)',
  setTableTool: 'table:painter/draw/eraser (disabled ids) AND the Border Styles menu: picking a style sets the pen but does not arm the Border Painter (ED-105)',
  hasCellsClip: 'paste:cells:* (disabled ids)',
  pasteCells: 'paste:cells:* (disabled ids)',
  quickTables: 'the Quick Tables submenu of the Table menu: the TS list over the empty TS model; picking one is a no-op (D7 remainder)',
  insertQuickTable: 'the Quick Tables submenu: a no-op (D7 remainder)',
};

/** Um valor universal: chamável, toda propriedade é ele mesmo, vira 0/'' em contas (o editor Proxy do teste da frente O). */
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
    set() { return true; },
    apply() { return p; },
  });
  return p;
}

describe('wave 5 (S): rotas e handlers', () => {
  it('(a) todo público roteado da frente existe no elemento e tem handler', () => {
    const h = storiesHandlers(stub);
    const pub = new Set(PUBLIC_METHODS);
    for (const n of ROUTES_STORIES.public) {
      expect(pub.has(n), `${n} is not a public method`).toBe(true);
      expect(typeof h[n], `${n} has no handler`).toBe('function');
    }
    expect(ROUTES_STORIES.private).toEqual([]);
  });

  it('(b) os comandos roteados das barras só chamam métodos roteados ou da casca', () => {
    const files = ['chrome-commands.ts', 'chrome-table.ts', 'chrome-hf.ts'].map((f) => readFileSync(join(__dirname, f), 'utf8'));
    const ok = new Set([...ROUTED_PUBLIC, ...SHELL, ...ALLOWED]);
    const bad: string[] = [];
    let found = 0;
    for (const id of ROUTES_STORIES.commands) {
      const key = `id: '${id}'`;
      for (const src of files) {
        let at = src.indexOf(key);
        while (at >= 0) {
          found++;
          // o corpo da definição: até a próxima definição de comando (ou 1.500 caracteres)
          const next = src.indexOf("id: '", at + key.length);
          const body = src.slice(at, Math.min(next < 0 ? src.length : next, at + 1500));
          for (const m of body.matchAll(/\bed\.([A-Za-z]+)\(/g)) if (!ok.has(m[1]!)) bad.push(`${id}: ed.${m[1]}()`);
          at = src.indexOf(key, at + key.length);
        }
      }
    }
    // os ids literais (os de modelo — `cell:align:${id}`… — não têm `id: '…'` no fonte: o (b3) os roda um a um)
    const literal = ROUTES_STORIES.commands.filter((id) => !/^(cell:align|table:look|table:border):/.test(id));
    expect(found).toBeGreaterThanOrEqual(literal.length - 12);
    expect([...new Set(bad)]).toEqual([]);
  });

  it('(b2) os diálogos que os comandos roteados abrem só chamam métodos roteados ou da casca', () => {
    const ok = new Set([...ROUTED_PUBLIC, ...SHELL, ...ALLOWED, 'isProtected']);
    const src = (f: string): string => readFileSync(join(__dirname, f), 'utf8');
    /** o corpo de uma função de nível de topo (até a próxima declaração de topo) */
    const body = (s: string, name: string): string => {
      const at = s.search(new RegExp(`\\n(export )?(async )?function ${name}\\b`));
      expect(at, `${name} not found`).toBeGreaterThanOrEqual(0);
      const next = s.slice(at + 1).search(/\n(export )?(async )?function |\nconst |\nexport const /);
      return s.slice(at, next < 0 ? s.length : at + 1 + next);
    };
    const bodies = [
      // os diálogos de tabela (Inserir Tabela, Converter, Propriedades, Classificar, Fórmula, Bordas e Sombreamento…): o arquivo todo
      src('chrome-table-dialogs.ts'),
      ...['notesDialog', 'convertNotesDialog', 'showNotesDialog'].map((n) => body(src('chrome-dialogs.ts'), n)),
      ...['pageNumberFormatDialog', 'hfOptionsDialog', 'dateTimeDialog', 'alignTabDialog'].map((n) => body(src('chrome-hf.ts'), n)),
    ];
    const bad = new Set<string>();
    let calls = 0;
    for (const b of bodies) for (const m of b.matchAll(/\bed\.([A-Za-z]+)\(/g)) { calls++; if (!ok.has(m[1]!)) bad.add(m[1]!); }
    expect(calls).toBeGreaterThan(30);
    expect([...bad]).toEqual([]);
  });

  it('(b3) todo comando registrado por chrome-table.ts e chrome-hf.ts é roteado ou está no resto declarado', { timeout: 60_000 }, async () => {
    const { registerTableCommands } = await import('./chrome-table.js');
    const { registerHfCommands } = await import('./chrome-hf.js');
    const cmds = new Map<string, { run: () => unknown }>();
    const reg = { add: (...cs: { id: string; run: () => unknown }[]) => { for (const c of cs) cmds.set(c.id, c); } };
    const log: string[] = [];
    const ed = universal(log, 'ed.'), app = universal(log, 'app.'), ctl = universal(log, 'ctl.');
    registerTableCommands(reg as never, ed as never, app as never, ctl as never);
    registerHfCommands(reg as never, ed as never, app as never, ctl as never);
    const routed = new Set([...ROUTES_STORIES.commands, ...ROUTES_OBJECTS.commands, ...ROUTES_REVIEW.commands]);
    const ok = new Set([...ROUTED_PUBLIC, ...SHELL, ...ALLOWED]);
    const off = [...cmds.keys()].filter((id) => !routed.has(id)).sort();
    // o resto declarado: exatamente estes ids ficam desligados (nem um a mais, nem um a menos)
    expect(off).toEqual(Object.keys(DISABLED_IDS).sort());
    // um id desligado cujo run só chamaria métodos roteados (e só lê o estado da barra) seria roteável: falha
    const routable: string[] = [];
    const bad: string[] = [];
    for (const [id, c] of cmds) {
      log.length = 0;
      try { const r = c.run(); void Promise.resolve(r).catch(() => undefined); } catch { /* o valor universal não serve a todo run */ }
      const eds = log.filter((x) => x.startsWith('ed.')).map((x) => x.slice(3));
      const ctls = log.filter((x) => x.startsWith('ctl.') && x !== 'ctl.state' && x !== 'ctl.flash');
      if (routed.has(id)) { for (const m of eds) if (!ok.has(m)) bad.push(`${id}: ed.${m}()`); }
      else if (eds.length && eds.every((m) => ok.has(m)) && !ctls.length) routable.push(id);
    }
    expect(bad).toEqual([]);
    expect(routable).toEqual([]);
    // os ids de modelo (ALIGN, LOOK, BORDERS) estão na tabela de rotas
    for (const p of ['cell:align:', 'table:look:', 'table:border:']) {
      const reg = [...cmds.keys()].filter((id) => id.startsWith(p));
      expect(reg.length, p).toBeGreaterThan(5);
      for (const id of reg) expect(ROUTES_STORIES.commands.includes(id), id).toBe(true);
    }
  });

  it('(b4) as chamadas ed.X( de chrome-table.ts e chrome-hf.ts inteiros (menus, galerias, widgets): roteadas, da casca ou declaradas', () => {
    const ok = new Set([...ROUTED_PUBLIC, ...SHELL, ...ALLOWED]);
    const seen = new Set<string>();
    for (const f of ['chrome-table.ts', 'chrome-hf.ts']) {
      for (const m of readFileSync(join(__dirname, f), 'utf8').matchAll(/\bed\.([A-Za-z]+)\(/g)) if (!ok.has(m[1]!)) seen.add(m[1]!);
    }
    // cada uma desligada com engine="rust" e declarada (registro "Wave 5 — S", D7 / ED-105)
    expect([...seen].sort()).toEqual(Object.keys(DISABLED_CALLS).sort());
  });

  it('(c) sem sessão os handlers respondem o neutro do TS', () => {
    const h = storiesHandlers(stub) as Record<string, (...a: unknown[]) => unknown>;
    expect(h['editingTableCell']!()).toBe(false);
    expect(h['editingHeaderFooter']!()).toBeNull();
    expect(h['editingTextbox']!()).toBe(false);
    expect(h['editingNote']!()).toBeNull();
    expect(h['getTableState']!()).toBeNull();
    expect(h['sortInfo']!()).toBeNull();
    expect(h['textToTableGuess']!()).toBeNull();
    expect(h['getHeaderFooterOptions']!()).toBeNull();
    expect(h['getPageNumberFormat']!()).toBeNull();
    expect(h['noteCount']!('footnote')).toBe(0);
    expect(h['canInsertNote']!()).toBe(false);
    expect(h['suggestFormula']!()).toBe('=');
    expect(h['previewFormula']!('=1')).toBe('');
    expect(h['insertFootnote']!()).toBeNull();
    expect(h['sort']!({ keys: [{ column: 0, type: 'text' }] })).toBe(false);
    expect(h['convertTextToTable']!()).toBe(false);
    expect(h['openNote']!('footnote', 1)).toBe(false);
    expect(h['documentTextShown']!()).toBe(true);
    expect(h['paintHeaderFooterTemplate']!()).toBe(false);
    // os mutadores não lançam
    for (const n of ['insertTable', 'deleteRow', 'mergeCells', 'enterHeaderFooter', 'exitHeaderFooter', 'setDifferentFirstPage', 'nextHeaderFooter', 'exitTextbox', 'setNoteOptions', 'convertNotes']) {
      expect(() => h[n]!('header', 0)).not.toThrow();
    }
  });
});
