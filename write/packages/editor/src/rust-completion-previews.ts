// Wave 7 (A2) of the editor switch (record core/docs/modules/uo-bindings.md "Wave 7 — A2: lists, paste, tables, objects and
// the page"): the previews the TS lays out in a mini-document and paints into a canvas — the cover and watermark galleries
// (`paintCoverTemplate`, `paintWatermarkTemplate`: `paintPreviewPage`), the table style thumbnails and the New/Modify Table
// Style dialog's preview (`renderTableStyleThumb`, `renderTableStyleXmlThumb`: `paintStyleThumb`) and the PNG of a Paste as
// Picture (`renderClipPng`, the session's `renderClip` job). The session builds the mini-document (uo-docx
// `session::ops::previews`) and lays it out (`uo_shell_preview`); the canvas, the crop and the scale are the TS code's.
import { uiLang } from '@universal-office/ui';
import { bindCompletion, type CompletionBinding, type WriteSession } from '@universal-office/write-session';
import type { FrontApi, RustEditor } from './rust-engine.js';
import { cstate, PX_PER_TWIP } from './rust-completion-core.js';
import { jobKinds, type JobOut } from './rust-objects-jobs.js';

type Kind = 'cover' | 'watermark' | 'tableStyleXml' | 'tableStyle' | 'renderClip' | 'headerFooter';
type Layout = ReturnType<FrontApi['module']['adopt']>;

const bindings = new WeakMap<object, CompletionBinding>();
function binding(api: FrontApi): CompletionBinding | null {
  const mod = api?.module;
  if (!mod) return null;
  let b = bindings.get(mod);
  if (!b) {
    try { b = bindCompletion(mod.exports, mod.binding); } catch { return null; }
    bindings.set(mod, b);
  }
  return b;
}

/** The preview's layout adopted (free it), or null (no session, no fonts yet, nothing to show). */
function previewLayout(api: FrontApi, kind: Kind, args: Record<string, unknown>): Layout | null {
  const s = api?.session, env = api?.pageEnv, b = binding(api);
  if (!s || !env || !b) return null;
  try {
    return api.module.adopt(b.preview(s as WriteSession, env.inner as never, kind, args, new Date()) as never);
  } catch {
    return null;
  }
}

const lang = (api: FrontApi): 'en' | 'pt' | 'es' => uiLang(api.el as unknown as HTMLElement);

/** `paintPreviewPage`'s canvas half (universal-write.ts 12142): the whole page at `cssWidth`, white. */
function paintPage(api: FrontApi, dl: Layout, canvas: HTMLCanvasElement, cssWidth: number): boolean {
  const page = dl.pages[0];
  if (!page) return false;
  const dpr = window.devicePixelRatio || 1;
  const scale = cssWidth / page.widthTw;
  canvas.width = Math.max(1, Math.round(page.widthTw * scale * dpr));
  canvas.height = Math.max(1, Math.round(page.heightTw * scale * dpr));
  canvas.style.width = `${Math.round(page.widthTw * scale)}px`;
  canvas.style.height = `${Math.round(page.heightTw * scale)}px`;
  const ctx = canvas.getContext('2d')!;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.fillStyle = '#ffffff';
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.setTransform(scale * dpr, 0, 0, scale * dpr, 0, 0);
  const penv = api.paintEnv(dl);
  if (penv) dl.paintItems(ctx, 0, penv, {});
  return true;
}

function pagePreview(api: FrontApi, kind: 'cover' | 'watermark', id: string, canvas: HTMLCanvasElement, cssWidth: number): boolean {
  const dl = previewLayout(api, kind, { id, lang: lang(api) });
  if (!dl) return false;
  try { return paintPage(api, dl, canvas, cssWidth); } catch { return false; } finally { dl.free(); }
}

/** `paintStyleThumb`'s canvas half (universal-write.ts 20220): the table centred, scaled to fit the canvas. */
function paintTableThumb(api: FrontApi, kind: 'tableStyleXml' | 'tableStyle', args: Record<string, unknown>, canvas: HTMLCanvasElement): void {
  const sample = kind === 'tableStyleXml';
  const W = sample ? 1440 * 4 : 1440 * 1.2, rowH = sample ? 300 : 150;
  const dl = previewLayout(api, kind, args);
  if (!dl) return;
  try {
    const ctx = canvas.getContext('2d');
    if (!dl.pages[0] || !ctx) return;
    const scale = Math.min(canvas.width / (W + 240), canvas.height / (rowH * 5 + 240));
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.setTransform(scale, 0, 0, scale, -120 * scale + (canvas.width - W * scale) / 2, -120 * scale + (canvas.height - rowH * 5 * scale) / 2);
    const penv = api.paintEnv(dl);
    if (penv) dl.paintItems(ctx, 0, penv, {});
    ctx.setTransform(1, 0, 0, 1, 0, 0);
  } catch { /* the TS paints nothing on a failed layout */ } finally { dl.free(); }
}

/** Front S's `renderTableStyleThumb` (A2 owns the table parts of rust-engine-stories.ts): the gallery thumbnail of `name`. */
export function completionTableStyleThumb(ed: RustEditor, canvas: HTMLCanvasElement, name: string, look?: unknown): Promise<void> {
  const api = cstate(ed).api;
  if (api) paintTableThumb(api, 'tableStyle', { name, ...(look ? { look } : {}) }, canvas);
  return Promise.resolve();
}

/**
 * Front S's `paintHeaderFooterTemplate(gallery, id, canvas, cssWidth)` (universal-write.ts 18521; A2 by decision 5 of the
 * record, ED-106 closed): the template laid out on the page of the caret's section `section`, cropped as the TS does (the
 * header or footer band, the whole page for Page Margins, the first body line for Current Position).
 */
export function completionHfThumb(
  ed: RustEditor, gallery: string, id: string, canvas: HTMLCanvasElement, cssWidth: number,
  at: { section: number; marginTop?: number; marginBottom?: number; marginLeft?: number; marginRight?: number } | null,
): boolean {
  const api = cstate(ed).api;
  if (!api) return false;
  const dl = previewLayout(api, 'headerFooter', { gallery, id, lang: lang(api), section: at?.section ?? 0 });
  if (!dl) return false;
  try {
    const page = dl.pages[0];
    if (!page) return false;
    // the section's margins (the TS `sectionSectPr(bodyModel, sec)`: front S's page answer of the caret's page)
    const sp = { marginTopTwips: at?.marginTop ?? 1440, marginBottomTwips: at?.marginBottom ?? 1440, marginLeftTwips: at?.marginLeft ?? 1440, marginRightTwips: at?.marginRight ?? 1440 };
    const current = gallery === 'pageCurrent';
    const kind = gallery === 'footer' || gallery === 'pageBottom' ? 'footer' : 'header';
    let top = 0, bottom = page.heightTw;
    if (gallery === 'pageMargin') { /* the whole page */ } else if (current) {
      const l = page.lines[0];
      top = Math.max(0, (l?.y ?? sp.marginTopTwips) - 240); bottom = top + (l?.height ?? 300) + 480;
    } else if (kind === 'header') { top = 0; bottom = Math.max(sp.marginTopTwips, ...page.lines.filter((l) => l.y < page.heightTw / 2).map((l) => l.y + l.height)) + 120; }
    else { top = Math.min(page.heightTw - sp.marginBottomTwips, ...page.lines.filter((l) => l.y > page.heightTw / 2).map((l) => l.y)) - 120; bottom = page.heightTw; }
    const left = gallery === 'pageMargin' ? 0 : Math.max(0, sp.marginLeftTwips - 360);
    const right = gallery === 'pageMargin' ? page.widthTw : Math.min(page.widthTw, page.widthTw - sp.marginRightTwips + 360);
    const dpr = window.devicePixelRatio || 1;
    const scale = cssWidth / (right - left);
    const hTw = bottom - top;
    canvas.width = Math.max(1, Math.round((right - left) * scale * dpr));
    canvas.height = Math.max(1, Math.round(hTw * scale * dpr));
    canvas.style.width = `${Math.round((right - left) * scale)}px`;
    canvas.style.height = `${Math.round(hTw * scale)}px`;
    const ctx = canvas.getContext('2d')!;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = (api.el as unknown as { pageDark?: boolean }).pageDark ? '#1e1e1e' : '#ffffff';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.setTransform(scale * dpr, 0, 0, scale * dpr, -left * scale * dpr, -top * scale * dpr);
    const penv = api.paintEnv(dl);
    if (penv) dl.paintItems(ctx, 0, penv, {});
    return true;
  } catch { return false; } finally { dl.free(); }
}

/** `headerFooterTemplateXml(gallery, id)` 11534: the template's XML (the theme's accent, the part style, random drawing ids). */
export function completionHfTemplateXml(ed: RustEditor, gallery: string, id: string): string | null {
  const c = cstate(ed), s = c.api?.session, b = c.api && binding(c.api);
  if (!s || !b) return null;
  try { return b.hfTemplateXml(s as WriteSession, gallery, id, lang(c.api)); } catch { return null; }
}

export function previewHandlers(ed: RustEditor): Record<string, (...args: never[]) => unknown> {
  const c = cstate(ed);
  return {
    paintCoverTemplate: ((id: string, canvas: HTMLCanvasElement, cssWidth: number): boolean => pagePreview(c.api, 'cover', id, canvas, cssWidth)) as never,
    sectPrAt: ((k: number) => {
      const c2 = cstate(ed), s = c2.api?.session, b = c2.api && binding(c2.api);
      if (!s || !b) return undefined;
      try { return b.sectPrAt(s as WriteSession, k) ?? undefined; } catch { return undefined; }
    }) as never,
    headerFooterTemplateXml: ((gallery: string, id: string): string | null => completionHfTemplateXml(ed, gallery, id)) as never,
    paintWatermarkTemplate: ((id: string, canvas: HTMLCanvasElement, cssWidth: number): boolean =>
      pagePreview(c.api, 'watermark', id, canvas, cssWidth)) as never,
    renderTableStyleXmlThumb: ((canvas: HTMLCanvasElement, styleXml: string, look?: unknown): void => {
      paintTableThumb(c.api, 'tableStyleXml', { xml: styleXml, lang: lang(c.api), ...(look ? { look } : {}) }, canvas);
    }) as never,
  };
}

/**
 * The session's `renderClip` job (a Paste as Picture): `renderClipPng(paras)` (universal-write.ts 5641) — the clip laid out
 * on the cursor section's page (at least 31,680 twips high), cropped to its lines and the text column, painted at 2 px per
 * 96-dpi pixel on white, encoded as PNG. No line: `failed` (the TS `null`: nothing is pasted).
 */
jobKinds.set('renderClip', async (_job, api): Promise<JobOut> => {
  const s = api?.session, b = api && binding(api);
  if (!s || !b) return { t: 'failed' };
  const m = b.renderClipMargins(s as WriteSession);
  const dl = m ? previewLayout(api, 'renderClip', {}) : null;
  if (!dl || !m) return { t: 'failed' };
  let canvas: HTMLCanvasElement;
  let w = 0, h = 0;
  try {
    const page = dl.pages[0];
    if (!page?.lines.length) return { t: 'failed' };
    const top = Math.min(...page.lines.map((l) => l.y)), bottom = Math.max(...page.lines.map((l) => l.y + l.height));
    const left = m.left, right = page.widthTw - m.right;
    const scale = PX_PER_TWIP * 2;
    canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round((right - left) * scale));
    canvas.height = Math.max(1, Math.round((bottom - top) * scale));
    const ctx = canvas.getContext('2d')!;
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.setTransform(scale, 0, 0, scale, -left * scale, -top * scale);
    const penv = api.paintEnv(dl);
    if (penv) dl.paintItems(ctx, 0, penv, {});
    w = right - left;
    h = bottom - top;
  } finally {
    dl.free();
  }
  const blob = await new Promise<Blob | null>((res) => canvas.toBlob(res, 'image/png'));
  if (!blob) return { t: 'failed' };
  return { t: 'rendered', png: new Uint8Array(await blob.arrayBuffer()), widthTwips: w, heightTwips: h };
});
