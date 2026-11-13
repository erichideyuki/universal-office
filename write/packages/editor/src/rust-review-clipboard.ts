// Frente R da leva 5 do engine="rust": a área de transferência SEM evento (os botões e itens de menu Recortar, Copiar,
// Colar, Colar sem formatação, Colar Especial, o painel da Área de Transferência do Office, os padrões do Colar) e as
// teclas com Alt que o TS roda (registro uo-bindings.md "Wave 5 — R", itens 12 e 16 do portão). O caminho do TS:
// `navigator.clipboard` (write/read), a cópia interna da sessão (`withRich`) quando a leitura é negada.
import { encodePaste } from '@universal-office/write-session';
import { fragmentOf } from './paste-html.js';
import { Ctx, storyCtx } from './rust-review-core.js';
import { jobKinds, runJobs, type JobOut } from './rust-objects-jobs.js';
import { completionPasteLink, completionRingKinds } from './rust-engine-completion.js'; // wave 7 (A2)

type Handlers = Record<string, (...args: never[]) => unknown>;
type PasteDefaults = { within: string; between: string; other: string };

const DEFAULTS: PasteDefaults = { within: 'keep', between: 'keep', other: 'default' };

/** O `writeClipboard` do TS: texto + HTML pelo ClipboardItem quando há HTML, senão `writeText`; sem permissão: nada. */
async function writeClipboard(text: string, html?: string): Promise<void> {
  try {
    const CI = (globalThis as { ClipboardItem?: typeof ClipboardItem }).ClipboardItem;
    if (html && CI && navigator.clipboard?.write) {
      await navigator.clipboard.write([new CI({ 'text/plain': new Blob([text], { type: 'text/plain' }), 'text/html': new Blob([html], { type: 'text/html' }) })]);
      return;
    }
    await navigator.clipboard?.writeText(text);
  } catch { /* sem permissão */ }
}

/** O `readClipboard` do TS: texto, HTML e imagem do sistema; sem permissão: null (a sessão cola a cópia interna). */
async function readClipboard(): Promise<{ text: string; html?: string; image?: { ext: string; data: Uint8Array } } | null> {
  const d: { text: string; html?: string; image?: { ext: string; data: Uint8Array } } = { text: '' };
  try {
    const cb = navigator.clipboard;
    if (cb?.read) {
      for (const it of await cb.read()) {
        if (it.types.includes('text/plain')) d.text = await (await it.getType('text/plain')).text();
        if (it.types.includes('text/html')) d.html = await (await it.getType('text/html')).text();
        const img = it.types.find((t) => /^image\/(png|jpeg|gif|bmp|webp)$/.test(t));
        // the TS button path's extension (`img.slice(6).replace('jpeg', 'jpg')`)
        if (img) d.image = { data: new Uint8Array(await (await it.getType(img)).arrayBuffer()), ext: img.slice(6).replace('jpeg', 'jpg') };
      }
    } else d.text = (await cb?.readText()) ?? '';
  } catch { return null; }
  return d;
}

/** The session's `readClipboard` job (Paste Special, a `paste` without data): the system clipboard as the TS reads it; denied:
 *  `failed` (the session pastes its own copy, the TS `richClip` fallback). Registered in frontier O's job runner. */
jobKinds.set('readClipboard', async (): Promise<JobOut> => {
  const d = await readClipboard();
  if (!d) return { t: 'failed' };
  let tree: Uint8Array | undefined;
  if (d.html) { try { tree = encodePaste(d.html, fragmentOf).bytes; } catch { tree = undefined; } }
  return { t: 'clipboard', text: d.text, ...(tree ? { htmlTree: tree } : {}), ...(d.image ? { image: d.image } : {}) };
});

export function clipboardHandlers(c: Ctx): { handlers: Handlers; adopt: () => void; installDefaults: () => void; notify: () => void; pushDefaults: () => void } {
  let defaults: PasteDefaults = { ...DEFAULTS };
  /** the last defaults the session got (the bars mutate the object in place: `Object.assign(ed.pasteDefaults, …)`) */
  let pushed = '';
  const pushDefaults = (): void => {
    if (!c.s) return;
    const now = JSON.stringify(defaults);
    if (now === pushed) return;
    pushed = now;
    c.raw({ t: 'clipboard', v: { t: 'setPasteDefaults', defaults } });
  };
  const listeners = new Set<() => void>();
  const notify = (): void => { for (const f of listeners) f(); };
  /** the TS `copy()`: the session decides what is copied (a drawing object, selected cells, the ranges of a non-contiguous
   *  selection, the text; a note reference or an object alone) and puts it in the ring; nothing copied: nothing written */
  const copy = async (): Promise<void> => {
    const s = c.s;
    if (!s) return;
    s.command({ t: 'clipboard', v: { t: 'copy' } });
    const cp = s.copied();
    if (!cp) return;
    notify();
    await writeClipboard(cp.text, cp.html || undefined);
  };
  /** paste with `mode` (absent: the session's default for the source; the TS `pasteDefaults[pasteSource(d)]`) */
  const paste = async (mode?: string): Promise<void> => {
    if (!c.s || c.locked()) return;
    const d = await readClipboard();
    if (!c.s) return;
    pushDefaults();
    let tree: Uint8Array | undefined;
    if (d?.html) {
      try { tree = encodePaste(d.html, fragmentOf).bytes; } catch (err) { c.api?.warnOnce('paste-tree', '[universal-write] engine="rust": the HTML paste tree failed (pasting the text)', err); }
    }
    // no read permission: the session's own copy (`withRich` with an empty text; the TS `richClip` fallback)
    const text = d?.text ?? (c.s.copied()?.text ?? '');
    // an image: the session's decode job (the shell's `apply` hands the jobs to frontier O's runner)
    c.run({ t: 'clipboard', v: { t: 'pasteData', text, ...(tree ? { html: tree } : {}), ...(d?.image ? { image: d.image } : {}), rich: 'withRich', ...(mode ? { mode } : {}), layout: {} } }, 'paste');
  };
  const items = (): { id: number; text: string; kind: string; at: number }[] => {
    const r = c.q<{ version: number; items: { id: number; text: string; at: { utc: { year: number; month: number; day: number; hour: number; minute: number } } }[] }>({ t: 'clipboardItems' });
    // wave 7 (A2): the kind of each item from the ring (drawing, table, image, text; ED-305's 'text' only)
    return completionRingKinds(c.api, (r?.items ?? []).map((x) => ({ id: x.id, text: x.text, kind: 'text', at: Date.UTC(x.at.utc.year, x.at.utc.month - 1, x.at.utc.day, x.at.utc.hour, x.at.utc.minute) })));
  };

  const handlers: Handlers = {
    copy: (() => copy()) as never,
    /** the TS `cut()`: the session's cut (a drawing object, the cells' rules, the text) copies first; then the system
     *  clipboard gets the copy (the TS awaits the write before deleting: the same end state) */
    cut: (async () => {
      const s = c.s;
      if (!s) return;
      c.run({ t: 'clipboard', v: { t: 'cut' } }, 'cut');
      const cp = s.copied();
      if (!cp) return;
      notify();
      await writeClipboard(cp.text, cp.html || undefined);
    }) as never,
    paste: ((mode?: string) => paste(mode)) as never,
    pastePlain: (() => paste('text')) as never,
    pasteSpecialFormats: (async () => {
      const s = c.s, api = c.api;
      if (!s || !api) return { formats: [], link: false, source: '' };
      // two-phase: the session asks for the system clipboard (`readClipboard`, answered below), then keeps it for
      // `pasteSpecial` (the TS `specialData`); a job left unanswered would hold every later command
      const o = s.command({ t: 'clipboard', v: { t: 'pasteSpecialFormats' } });
      if (o?.t === 'jobs') await runJobs(api, o, 'pasteSpecialFormats', false);
      if (c.s !== s) return { formats: [], link: false, source: '' };
      const v = c.q<{ value?: { formats: string[]; link?: boolean; source?: string } }>({ t: 'pasteSpecialFormats' })?.value;
      return { formats: v?.formats ?? [], link: !!v?.link, source: v?.source ?? '' };
    }) as never,
    pasteSpecial: (async (format: string, o: { link?: boolean } = {}) => {
      if (!c.s || c.locked()) return;
      if (completionPasteLink(c.ed, format, o)) return; // wave 7 (A2, decision 3): a Paste Link whose copy came from another editor
      c.run({ t: 'clipboard', v: { t: 'pasteSpecial', format, link: !!o.link, layout: {} } }, 'pasteSpecial');
    }) as never,
    /** ED-305: the ring of THIS session (the TS ring is page-global) */
    clipboardItems: (() => items()) as never,
    pasteClipboardItem: ((id: number, mode?: string) => { pushDefaults(); c.run({ t: 'clipboard', v: { t: 'pasteClipboardItem', id, ...(mode ? { mode } : {}), layout: {} } }, 'pasteClipboardItem'); }) as never,
    pasteAllClipboardItems: (() => { pushDefaults(); c.run({ t: 'clipboard', v: { t: 'pasteAllClipboardItems', layout: {} } }, 'pasteAllClipboardItems'); }) as never,
    removeClipboardItem: ((id: number) => { c.run({ t: 'clipboard', v: { t: 'removeClipboardItem', id } }, 'removeClipboardItem'); notify(); }) as never,
    clearClipboardItems: (() => { c.run({ t: 'clipboard', v: { t: 'clearClipboardItems' } }, 'clearClipboardItems'); notify(); }) as never,
    onClipboardChange: ((f: () => void) => { listeners.add(f); return () => { listeners.delete(f); }; }) as never,
  };

  /** `pasteDefaults` is a public FIELD of the element: an accessor that pushes it to the session (decision 7) */
  const installDefaults = (): void => {
    const el = c.el;
    if (!el) return;
    const own = Object.getOwnPropertyDescriptor(el, 'pasteDefaults');
    if (own?.value && typeof own.value === 'object') defaults = { ...DEFAULTS, ...(own.value as PasteDefaults) };
    Object.defineProperty(el, 'pasteDefaults', {
      configurable: true,
      enumerable: true,
      get: () => defaults,
      set: (v: PasteDefaults) => { defaults = { ...DEFAULTS, ...v }; pushDefaults(); },
    });
  };
  /** a new session: its defaults are the element's again */
  const adopt = (): void => { pushed = ''; pushDefaults(); };
  return { handlers, adopt, installDefaults, notify, pushDefaults };
}

/**
 * The TS `bindEvents` keys (universal-write.ts 10516–10790) the wave-4 shell left to the bars or prevented without running
 * (ED-21, gate item 16): every key with Alt, Ctrl+PageUp/PageDown and Ctrl+Tab (the TS rules take any modifier), F9 and
 * Ctrl+hyphen. `k` (the key) is read as the TS reads it: AltGr characters whose key is not a letter of the Ctrl map still
 * type (ED-308). The rules run in the TS order; the stories' and objects' keys (frontiers S and O) ran before this hook.
 */
export function keyHook(c: Ctx, h: Handlers, pushDefaults: () => void): (e: KeyboardEvent) => boolean {
  const el = (): Record<string, (...a: unknown[]) => unknown> => c.el as unknown as Record<string, (...a: unknown[]) => unknown>;
  const sync = (): void => { const x = c.el; x?.onSelectionChange?.(); x?.chromeSync?.(); };
  const ed = (): Record<string, (...a: unknown[]) => unknown> => c.ed as unknown as Record<string, (...a: unknown[]) => unknown>;
  return (e: KeyboardEvent): boolean => {
    if (!c.s) return false;
    const ctrl = e.ctrlKey || e.metaKey;
    const k = e.key;
    const lk = k.toLowerCase();
    const done = (f: () => void): boolean => { e.preventDefault(); f(); sync(); return true; };
    const call = (m: string, ...a: unknown[]): unknown => el()[m]?.(...a);
    // F9: the fields of the selection (a TOC in it: the bar's question, `uw-toc-update`; ED-317)
    if (k === 'F9' && !ctrl && !e.altKey && !e.shiftKey) {
      return done(() => {
        if (c.q<boolean>({ t: 'inTableOfContents' })) {
          const ev = new CustomEvent('uw-toc-update', { cancelable: true, detail: { scope: 'selection' } });
          if (!c.el!.dispatchEvent(ev)) return;
        }
        call('updateFields', 'selection');
      });
    }
    // Ctrl+V / Ctrl+Shift+V: the paste event comes next and reads the session's defaults (the bars' Paste Options
    // mutate `el.pasteDefaults` in place): pushed now; the shell goes on
    if (ctrl && !e.altKey && e.code === 'KeyV') { pushDefaults(); return false; }
    // Ctrl+hyphen = soft hyphen; Ctrl+Shift+hyphen = non-breaking hyphen (by the key's code)
    if (ctrl && !e.altKey && (e.code === 'Minus' || e.code === 'NumpadSubtract')) {
      return done(() => { if (!c.locked()) call('pasteText', e.shiftKey ? '‑' : '­'); });
    }
    const docEdge = (): void => {
      // the TS `ctrl && (Home || End)` (no Alt check): the start/end of the document; with Shift the selection extends
      // (in a cell without Shift, frontier S's hook left the cell first)
      const info = c.s!.storyInfo();
      const to = k === 'Home' ? { para: 0, offset: 0 } : { para: Math.max(0, info.paras - 1), offset: info.lastLength };
      ed()['setSelection']?.call(c.ed, e.shiftKey ? c.el!.selection.anchor : to, to);
    };
    const page = (): void => {
      // the TS PageUp/PageDown (any modifier): 0.9 of the view, then ~one screen of line moves
      const host = c.el as unknown as HTMLElement;
      host.scrollTop += host.clientHeight * 0.9 * (k === 'PageUp' ? -1 : 1);
      for (let i = 0; i < 20; i++) call('moveCaret', k === 'PageUp' ? 'up' : 'down', e.shiftKey);
    };
    // the shell runs the plain ones; with Ctrl or Alt the TS rules
    if ((k === 'PageUp' || k === 'PageDown') && (ctrl || e.altKey)) return done(page);
    // Tab with Ctrl or Alt: the TS Tab rule (a list level, else a tab character; a cell: frontier S, without Ctrl)
    if (k === 'Tab' && (ctrl || e.altKey)) return done(() => { ed()['tab']?.call(c.ed, e.shiftKey); });
    if (!e.altKey) return false;
    if (ctrl) {
      // Ctrl+Alt+F/D (notes): frontier S's hook ran first
      if (!e.shiftKey && (lk === 'f' || lk === 'd')) return false;
      if (k === 'ArrowLeft' || k === 'ArrowRight') return done(() => call('moveWord', k === 'ArrowRight' ? 1 : -1, e.shiftKey));
      if (k === 'Backspace' || k === 'Delete') return done(() => call('deleteWord', k === 'Delete' ? 1 : -1));
      if (lk === 'c') return done(() => { void (h['copy'] as () => Promise<void>)(); });
      if (lk === 'x') return done(() => { void (h['cut'] as () => Promise<void>)(); });
      if (lk === 'v' && e.code === 'KeyV') return done(() => { c.el!.dispatchEvent(new CustomEvent('uw-paste-special')); });
      const map: Record<string, () => void> = {
        b: () => call('toggleRunProp', 'bold'), i: () => call('toggleRunProp', 'italic'), u: () => call('toggleRunProp', 'underline'),
        z: () => call('undo'), y: () => call('redo'), a: () => call('selectAll'),
        e: () => call('setAlignment', 'center'), l: () => call('setAlignment', 'left'), r: () => call('setAlignment', 'right'), j: () => call('setAlignment', 'both'),
      };
      const fn = map[lk];
      if (fn) return done(() => { if (c.locked() && lk !== 'a' && lk !== 'z' && lk !== 'y') return; fn(); });
      if (k === 'Home' || k === 'End') return done(docEdge);
      // Ctrl+Alt+Enter = page break, Ctrl+Alt+Shift+Enter = column break (the TS nav map: `ctrl` without an Alt check);
      // in a note nothing (as the Word)
      if (k === 'Enter') {
        return done(() => {
          if (storyCtx(c)?.kind === 'note') return;
          call(e.shiftKey ? 'insertColumnBreak' : 'insertPageBreak');
        });
      }
      // Ctrl+Alt+Up/Down: the TS nav map (a line)
      if (k === 'ArrowUp' || k === 'ArrowDown') return done(() => call('moveCaret', k === 'ArrowUp' ? 'up' : 'down', e.shiftKey));
      return false;
    }
    const nav: Record<string, () => void> = {
      ArrowLeft: () => call('moveCaret', 'left', e.shiftKey), ArrowRight: () => call('moveCaret', 'right', e.shiftKey),
      ArrowUp: () => call('moveCaret', 'up', e.shiftKey), ArrowDown: () => call('moveCaret', 'down', e.shiftKey),
      Home: () => call('moveCaret', 'home', e.shiftKey), End: () => call('moveCaret', 'end', e.shiftKey),
      Enter: () => call('pressEnter'),
      Backspace: () => { if (!c.locked()) c.api!.apply(c.s!.backspace(), 'backspace'); },
      Delete: () => { if (!c.locked()) c.api!.apply(c.s!.delete(), 'delete'); },
    };
    const fn = nav[k];
    return fn ? done(fn) : false;
  };
}
