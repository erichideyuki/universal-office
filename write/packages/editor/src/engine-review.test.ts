// As rotas da frente R da leva 5 do engine="rust" (registro core/docs/modules/uo-bindings.md "Wave 5 — R"): cada nome
// público de ROUTES_REVIEW é um método público do elemento e de nenhuma outra frente; cada privado existe no protótipo;
// cada id de comando existe nas barras; sem sessão (o motor falso do teste de rotas, ou antes do 1º documento) cada
// handler responde o neutro sem lançar, e `currentStyles` delega ao do RustEditor (o teste de rotas registra a chamada).
// A paridade de verdade (Chromium, as duas máquinas) é core/crates/uo-wasm/tests/browser/editor_switch_review.mjs.
import { describe, expect, it } from 'vitest';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { PUBLIC_METHODS } from './engine-public.js';
import { ROUTES_OBJECTS } from './engine-routes-objects.js';
import { ROUTES_REVIEW } from './engine-routes-review.js';
import { ROUTES_STORIES } from './engine-routes-stories.js';
import { ROUTED_COMMAND_IDS, ROUTED_PRIVATE, ROUTED_PUBLIC } from './engine-routes.js';
import { reviewHandlers } from './rust-engine-review.js';
import type { Ctx } from './rust-review-core.js';
import { authorSync } from './rust-review-revisions.js';
import { SessionSpell } from './rust-review-spell.js';
import type { SpellEngine } from './spell.js';
import type { RustEditor } from './rust-engine.js';

const src = readFileSync(join(__dirname, 'universal-write.ts'), 'utf8');
const chrome = readdirSync(__dirname).filter((f) => /^chrome-.*\.ts$/.test(f) && !f.endsWith('.test.ts')).map((f) => readFileSync(join(__dirname, f), 'utf8')).join('\n');

describe('engine="rust", wave 5, frontier R: routes', () => {
  it('names: public methods of the element, private methods of the prototype, bar command ids; no overlap with S or O', () => {
    for (const n of ROUTES_REVIEW.public) {
      expect(PUBLIC_METHODS.includes(n), `public ${n}`).toBe(true);
      expect(ROUTED_PUBLIC.includes(n), `routed ${n}`).toBe(true);
      expect(ROUTES_STORIES.public.includes(n) || ROUTES_OBJECTS.public.includes(n), `${n} in another front`).toBe(false);
    }
    for (const n of ROUTES_REVIEW.private) {
      expect(new RegExp(`\\n {2}private ${n}\\(`).test(src), `private ${n}`).toBe(true);
      expect(ROUTED_PRIVATE.includes(n)).toBe(true);
    }
    for (const id of ROUTES_REVIEW.commands) {
      // the generated ids (`style:h${n}`, `markup:${m}`, `show:${k}`, `balloons:${b}`) are built from template parts
      const generated = /^(style:h\d|markup:|show:|balloons:)/.test(id);
      expect(generated || chrome.includes(`'${id}'`), `command ${id}`).toBe(true);
      expect(ROUTED_COMMAND_IDS.has(id)).toBe(true);
      expect(ROUTES_STORIES.commands.includes(id) || ROUTES_OBJECTS.commands.includes(id), `${id} in another front`).toBe(false);
    }
    console.log(`frontier R: ${ROUTES_REVIEW.public.length} public, ${ROUTES_REVIEW.private.length} private, ${ROUTES_REVIEW.commands.length} bar commands`);
  });

  it('without a session every handler answers the neutral value; currentStyles delegates', async () => {
    const calls: string[] = [];
    const fake = { currentStyles: () => { calls.push('currentStyles'); return { paragraph: 'Normal' }; } } as unknown as RustEditor;
    const h = reviewHandlers(fake) as Record<string, (...a: unknown[]) => unknown>;
    for (const n of [...ROUTES_REVIEW.public, ...ROUTES_REVIEW.private]) expect(typeof h[n], `handler ${n}`).toBe('function');
    const neutral: Record<string, unknown> = {
      getComments: [], getRevisions: [], isTrackingChanges: false, getMarkupMode: 'all', revisionAuthors: [], revisionAnchors: [],
      revisionAtCaret: null, addComment: null, listStyles: [], paragraphStyles: [], styleFormatting: null, getBookmarks: [],
      getHeadings: [], currentHeading: -1, canInsertField: false, hasTableOfContents: false, inTableOfContents: false,
      getLink: null, clipboardItems: [], spellingAtCaret: null, acceptAllChanges: 0, rejectAllChanges: 0, updateFields: 0,
      findNextMatch: { found: false }, search: [], highlightMatches: 0, selectAllMatches: 0, getSelectionRanges: [],
      nextChange: false, previousChange: false, selectRevision: false, insertBookmark: false, deleteBookmark: false,
      createStyle: null, inspectStyle: { paragraph: { id: 'Normal', name: 'Normal', direct: {} }, text: { id: '', name: '', direct: {} } },
    };
    for (const [n, v] of Object.entries(neutral)) expect(h[n]!(), n).toEqual(v);
    // the rest: no throw (a void answer, a resolved promise, or a neutral value)
    const ctx = { save() {}, restore() {}, fillRect() {}, stroke() {}, beginPath() {}, moveTo() {}, quadraticCurveTo() {}, transform() {} };
    for (const n of [...ROUTES_REVIEW.public, ...ROUTES_REVIEW.private]) {
      if (n in neutral || n === 'currentStyles' || n === 'onClipboardChange') continue;
      const r = h[n]!(ctx, 0);
      if (r instanceof Promise) await r;
    }
    expect(h['currentStyles']!()).toEqual({ paragraph: 'Normal' });
    expect(calls).toEqual(['currentStyles']);
    const off = h['onClipboardChange']!(() => {}) as () => void;
    expect(typeof off).toBe('function');
    off();
  });
});

describe('engine="rust", wave 5, frontier R: lifetimes (fix round 1)', () => {
  it('SessionSpell: after dispose no check reaches the shared checker and the listener is gone', async () => {
    let listeners = 0;
    const calls: string[] = [];
    let release: (() => void) | null = null;
    const engine: SpellEngine = {
      checkText: async (text) => { calls.push(text); await new Promise<void>((r) => { release = r; }); return []; },
      suggest: async () => [], add: async () => {}, ignore: () => {},
      resolve: (lang) => ({ id: lang, exact: true }),
      onChange: () => { listeners++; return () => { listeners--; }; },
      dispose: () => {},
    };
    const spell = new SessionSpell(engine, () => {}, 0);
    expect(listeners).toBe(1);
    // two languages: the second check would start after the first answers
    expect(spell.get([{ lang: 'pt-BR', text: 'um' }, { lang: 'en-US', text: 'two' }])).toBeUndefined();
    await new Promise((r) => setTimeout(r, 5));
    expect(calls).toEqual(['um']);
    spell.dispose();
    expect(listeners).toBe(0);
    release!();
    await new Promise((r) => setTimeout(r, 5));
    expect(calls).toEqual(['um']);
    expect(spell.get([{ lang: 'pt-BR', text: 'tres' }])).toBeUndefined();
    await new Promise((r) => setTimeout(r, 5));
    expect(calls).toEqual(['um']);
    expect(spell.pending).toBe(0);
  });

  it('authorSync: pushes the live author once per change (attribute observer, property, adopt)', async () => {
    const pushed: unknown[] = [];
    class Host extends EventTarget {
      private name = 'Ana';
      attr: string | null = null;
      get author(): string { return this.attr || this.name; }
      set author(v: string) { this.name = v; }
    }
    const el = new Host();
    const c = {
      el, s: {}, raw: (cmd: unknown) => { pushed.push(cmd); return null; },
    } as unknown as Ctx;
    const a = authorSync(c);
    a.install();
    a.adopt();
    a.sync();
    expect(pushed).toEqual([{ t: 'settings', v: { t: 'setAuthor', name: 'Ana' } }]);
    (el as unknown as { author: string }).author = 'Caio';
    expect(pushed.at(-1)).toEqual({ t: 'settings', v: { t: 'setAuthor', name: 'Caio' } });
    el.attr = 'Bia';
    a.sync();
    a.sync();
    expect(pushed.length).toBe(3);
    expect(pushed.at(-1)).toEqual({ t: 'settings', v: { t: 'setAuthor', name: 'Bia' } });
    // a new session: pushed again (decision 6)
    a.adopt();
    expect(pushed.length).toBe(4);
  });
});
