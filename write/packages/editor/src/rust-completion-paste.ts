// Wave 7 (A2): the paste family left by the wave-5 fronts (universal-write.ts 5089–5190, 20521, 15690): `pasteCells` (the
// session's `PasteCells` over the ring's current copy), `hasCellsClip` (the ring's), `pasteData` (the session's `PasteData`:
// text, HTML and image; a TS `RichClip` in `d.rich` only exists in a TS-engine element: ignored, declared), Change Picture
// › From Clipboard (the TS body: it calls the routed `changeImage`), the kind of each ring item, and the element's
// `documentPath` pushed to the session (Paste Special's link formats). Record uo-bindings.md "Wave 7 — A2".
import { encodePaste } from '@universal-office/write-session';
import { fragmentOf } from './paste-html.js';
import { ask, cb, command, cstate, locked, through, type CState } from './rust-completion-core.js';
import type { RustEditor } from './rust-engine.js';

type Handlers = Record<string, (...args: never[]) => unknown>;

/** The ring items with their kinds (`clipboardItems()` of the TS: drawing, table, image, text). */
export function completionClipKinds(c: CState, items: { id: number; kind: string }[]): void {
  const b = cb(c);
  if (!b) return;
  let kinds: Map<number, string>;
  try { kinds = b.clipboardKinds(); } catch { return; }
  for (const it of items) it.kind = kinds.get(it.id) ?? it.kind;
}

/** The element's `documentPath` (a public field of the TS element: the path Paste Link writes) pushed to the session when it
 *  changed (`ClipboardCmd::SetDocumentPath`). */
export function pushDocumentPath(c: CState): void {
  const el = c.ed.el as unknown as { documentPath?: string | null };
  const p = el.documentPath ?? '';
  if (!c.api?.session || p === (c.pushedPath ?? '')) return;
  c.pushedPath = p;
  c.api.session.command({ t: 'clipboard', v: { t: 'setDocumentPath', ...(p ? { path: p } : {}) } } as never);
}

/** `documentPath` is a public FIELD of the element: an accessor that pushes it to the session (as front R's
 *  `pasteDefaults`), so a copy carries the path the TS copy carries. */
export function installDocumentPath(c: CState): void {
  const el = c.ed.el as unknown as Record<string, unknown>;
  const own = Object.getOwnPropertyDescriptor(el, 'documentPath');
  if (own?.get) return;
  let path = (own?.value as string | undefined) ?? undefined;
  Object.defineProperty(el, 'documentPath', {
    configurable: true,
    enumerable: true,
    get: () => path,
    set: (v: string | undefined) => { path = v; pushDocumentPath(c); },
  });
  pushDocumentPath(c);
}

// ───────────────────────── Paste Link between two editors (pasteLink 5718, addLinkSourceBookmark 5741) ─────────────────────────

/** The completion states of the page's Rust editors (the TS `clip.src.el`: the element a copy came from). */
const live = new Set<WeakRef<CState>>();

/** A Rust editor registered as a possible Paste Link source (on its first handlers). */
export function registerLinkSource(c: CState): void {
  for (const r of live) if (r.deref() === c) return;
  live.add(new WeakRef(c));
}

function editorOf(id: string): CState | null {
  for (const r of [...live]) {
    const x = r.deref();
    if (!x) { live.delete(r); continue; }
    const s = x.api?.session;
    if (!s || !(x.ed.el as unknown as { isConnected?: boolean }).isConnected) continue;
    const o = ask(x, 'linkOrigin', (b, ss) => b.linkOrigin(ss));
    if (o?.me === id) return x;
  }
  return null;
}

/**
 * Front R's `pasteSpecial(format, { link })` when the copy came from ANOTHER editor of the page (decision 3: one additive call
 * in rust-review-clipboard.ts): the TS `pasteLink(clip, text)` — the source marks the copied range with its `OLE_LINKn`
 * bookmark (`ClipboardCmd::AddLinkSource` on the source's session), then this session writes the `LINK` field around the copy
 * (`PasteLinkWith`). True when handled (also when the TS pastes nothing: the source is gone or its range changed); false
 * for R's own path (the same session, no link).
 */
export function completionPasteLink(ed: RustEditor, format: string, o: { link?: boolean } = {}): boolean {
  const c = cstate(ed);
  if (!o?.link || !['rtf', 'html', 'text', 'unicode'].includes(format) || !c.api?.session) return false;
  const lo = ask(c, 'linkOrigin', (b, s) => b.linkOrigin(s));
  if (!lo?.link || !lo.clip || lo.clip === lo.me) return false;
  const src = editorOf(lo.clip);
  const ss = src?.api?.session;
  if (!src || !ss) return true;
  src.api.command({ t: 'clipboard', v: { t: 'addLinkSource' } } as never, 'addLinkSource');
  let name: string | null = null;
  try {
    const a = ss.query({ t: 'linkSourceName' } as never) as Record<string, unknown>;
    name = (('v' in a ? a['v'] : a['value']) as string | null | undefined) ?? null;
  } catch { name = null; }
  if (!name) return true;
  command(c, { t: 'clipboard', v: { t: 'pasteLinkWith', name, text: format === 'text' || format === 'unicode', layout: {} } } as never, 'pasteSpecial');
  return true;
}

export function pasteHandlers(ed: RustEditor): Handlers {
  const c = cstate(ed);
  registerLinkSource(c);
  return {
    hasCellsClip: () => !!ask(c, 'hasCellsClip', (b) => b.hasCellsClip()),
    pasteCells: ((mode: 'overwrite' | 'rows' | 'nest' | 'text' = 'overwrite') => {
      // TS 20485: `!clip || !this.model || this.locked() || this.view === 'read'` (the read view is in `locked()`)
      if (locked(c) || (c.ed.el as unknown as { view?: string }).view === 'read' || !ask(c, 'hasCellsClip', (b) => b.hasCellsClip())) return false;
      return command(c, { t: 'clipboard', v: { t: 'pasteCells', mode, layout: {} } } as never, 'pasteCells');
    }) as never,
    pasteData: ((d: { text?: string; html?: string; image?: { data: Uint8Array; ext: string }; rich?: unknown }, mode?: string, list: string | null = null) => {
      if (locked(c) || !d) return;
      pushDocumentPath(c);
      let tree: Uint8Array | undefined;
      if (d.html) { try { tree = encodePaste(d.html, fragmentOf).bytes; } catch (err) { c.api.warnOnce('pasteData-tree', '[universal-write] engine="rust": the HTML paste tree failed (pasting the text)', err); } }
      if (d.rich) c.api.warnOnce('pasteData-rich', '[universal-write] engine="rust": pasteData ignores a TS RichClip (d.rich); the text, HTML and image are pasted');
      const v = {
        t: 'pasteData', text: d.text ?? '', ...(tree ? { html: tree } : {}), ...(d.image ? { image: { ext: d.image.ext, data: new Uint8Array(d.image.data) } } : {}),
        rich: 'none', ...(mode ? { mode } : {}), ...(list ? { list } : {}), layout: {},
      };
      command(c, { t: 'clipboard', v } as never, 'pasteData');
    }) as never,
    // the TS body reads `navigator.clipboard` and calls `this.changeImage` (routed: front O's two-phase change)
    changeImageFromClipboard: (() => through(c, 'changeImageFromClipboard', [])) as never,
  };
}
