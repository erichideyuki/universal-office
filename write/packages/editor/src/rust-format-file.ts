// Frente A1 da leva 7 do engine="rust": Arquivo (registro uo-bindings.md "Wave 7 — A1"). Abrir .odt (o leitor ODF de
// uo-docx), salvar/exportar .odt, HTML e Markdown (os exportadores de uo-docx sobre o documento da sessão; as imagens
// comprimidas pelo canvas do navegador como no TS, D-A1-7), XPS (o escritor de uo-pdf sobre as páginas do layout da
// sessão, D-A1-2), imprimir (o código TS do elemento sobre a pintura roteada, com as imagens decodificadas antes).
import { FormatCtx, type Handlers } from './rust-format-core.js';
import { storyCtx } from './rust-review-core.js';

/** `detectFormat` (universal-write.ts 2184): a zip whose first 2000 bytes name the ODF text mimetype. */
function isOdt(bytes: Uint8Array): boolean {
  if (bytes.length < 4 || bytes[0] !== 0x50 || bytes[1] !== 0x4b) return false;
  const head = new TextDecoder('latin1').decode(bytes.subarray(0, Math.min(2000, bytes.length)));
  return head.includes('application/vnd.oasis.opendocument.text');
}

const MIME: Record<string, string> = { jpg: 'image/jpeg', jpeg: 'image/jpeg', png: 'image/png', gif: 'image/gif', bmp: 'image/bmp', webp: 'image/webp' };
const mimeFor = (ext: string): string => MIME[ext.toLowerCase()] ?? `image/${ext.toLowerCase()}`;

function toBase64(bytes: Uint8Array): string {
  let bin = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) bin += String.fromCharCode(...bytes.subarray(i, i + chunk));
  return btoa(bin);
}

/**
 * `compressImagesForExport()` (universal-write.ts 8731, copied, declared): decode → at most 1200 px → JPEG 0.78 → data URI;
 * ≤ 32 KB passes through. The same browser canvas as the TS engine, so the same bytes.
 */
async function compress(images: { id: number; ext: string; data: Uint8Array }[]): Promise<[number, string][]> {
  const MAX_DIM = 1200, QUALITY = 0.78, PASSTHROUGH_KB = 32;
  return Promise.all(images.map(async (img): Promise<[number, string]> => {
    if (img.data.byteLength <= PASSTHROUGH_KB * 1024) return [img.id, `data:${mimeFor(img.ext)};base64,${toBase64(img.data)}`];
    try {
      const blob = new Blob([img.data.slice().buffer as ArrayBuffer], { type: mimeFor(img.ext) });
      const bmp = await createImageBitmap(blob);
      const scale = Math.min(1, MAX_DIM / Math.max(bmp.width, bmp.height));
      const w = Math.max(1, Math.round(bmp.width * scale)), h = Math.max(1, Math.round(bmp.height * scale));
      const canvas = document.createElement('canvas');
      canvas.width = w; canvas.height = h;
      const ctx = canvas.getContext('2d')!;
      ctx.fillStyle = '#fff'; ctx.fillRect(0, 0, w, h);
      ctx.drawImage(bmp, 0, 0, w, h);
      bmp.close?.();
      const uri: string = await new Promise((resolve, reject) => {
        canvas.toBlob((b) => {
          if (!b) { reject(new Error('canvas.toBlob returned null')); return; }
          const fr = new FileReader();
          fr.onload = () => resolve(fr.result as string);
          fr.onerror = () => reject(fr.error);
          fr.readAsDataURL(b);
        }, 'image/jpeg', QUALITY);
      });
      return [img.id, uri];
    } catch {
      return [img.id, `data:${mimeFor(img.ext)};base64,${toBase64(img.data)}`];
    }
  }));
}

export function fileHandlers(c: FormatCtx, prev: Record<string, unknown>): Handlers {
  const ed = c.ed as unknown as Record<string, (...a: unknown[]) => unknown>;
  const base = (name: string, args: unknown[]): unknown => {
    const f = prev[name] ?? ed[name];
    return typeof f === 'function' ? (f as (...a: unknown[]) => unknown).apply(prev[name] ? undefined : c.ed, args) : undefined;
  };
  const session = (): NonNullable<FormatCtx['s']> => {
    const s = c.s;
    if (!s) throw new Error('document not loaded');
    return s;
  };
  const exportText = async (markdown: boolean, title?: string, opts: { images?: 'data' | 'omit' } = {}): Promise<string> => {
    const s = session(), fb = c.fb;
    if (!fb) throw new Error('engine="rust": the export module is missing');
    const omit = opts.images === 'omit';
    const uris = omit ? [] : await compress(fb.images(s));
    if (c.s !== s) throw new Error('document changed during the export');
    return fb.exportText(s, markdown, title, omit, uris);
  };
  const exportOdt = (opts: { images?: 'embed' | 'omit' } = {}): Uint8Array => {
    const s = session(), fb = c.fb;
    if (!fb) throw new Error('engine="rust": the export module is missing');
    return fb.exportOdt(s, opts.images === 'omit');
  };
  const loadOdt = (bytes: Uint8Array, opts: { protected?: boolean } = {}): void => {
    const api = c.api, fb = c.fb;
    if (!api || !fb) return;
    const prot = opts.protected ?? false; // .odt has no MOTW: the default trust (the TS)
    const s = fb.openOdt(api.ss, bytes, { protected: prot, author: api.el.author, clock: new Date(), seed: 1 });
    api.adoptSession(s, null, prot);
  };
  /**
   * `exportXps()` (22640): the TS writes its legacy layout's pages; here the pages of the session's layout, written in Rust
   * (uo-pdf `xps`, D-A1-2: glyphs with their faces and placed advances, alphas, clips, paths, shapes, pictures). In the Web,
   * Draft and Outline views the shell engine lays out the pages for the export, then the view again (the TS export is
   * always of the pages).
   *
   * Wave 7 (F2) — the side effects of the TS `withLegacyLayout` (15996), measured in Chromium on both engines
   * (`editor_switch_format.mjs --mode extra`, "exportXps side effects"): in the Print view (the TS has its engine) it leaves a
   * cell being edited (`exitCell`), exports, then `refresh()`es — the date fields' clock moves (a new epoch here, D-LA7-24); in
   * the Web/Draft/Outline views the TS has no engine (`!this.eng`: the export only), so neither the cell nor the clock moves:
   * the pages are laid out and the view restored with `relayoutNow` (the epoch kept). Selection and dirty flag untouched.
   */
  const exportXps = (): Uint8Array => {
    const fb = c.fb, api = c.api;
    if (!c.s || !c.dl || !fb || !api) throw new Error('document not loaded');
    const e = c.el as unknown as { view: string } | null;
    const view = e?.view ?? 'print';
    const continuous = view === 'web' || view === 'draft' || view === 'outline';
    const ed = c.ed as unknown as { refresh(): void };
    if (!continuous || !c.viewApply) {
      if (storyCtx(c)?.kind === 'cell') c.run({ t: 'context', v: { t: 'exitCell' } } as never, 'exportXps');
      const dl = c.dl;
      if (!dl) throw new Error('no layout');
      try { return fb.xps(dl.inner); } finally { ed.refresh(); }
    }
    c.viewApply('print');
    try {
      api.relayoutNow();
      const dl = c.dl;
      if (!dl) throw new Error('no layout');
      return fb.xps(dl.inner);
    } finally {
      c.viewApply(view);
      api.relayoutNow();
    }
  };

  return {
    load: ((bytes: Uint8Array, format = 'auto', opts: { protected?: boolean } = {}) => {
      if (format === 'odt' || (format === 'auto' && isOdt(bytes))) { loadOdt(bytes, { protected: opts.protected ?? false }); return; }
      base('load', [bytes, format === 'auto' ? 'docx' : format, opts]);
    }) as never,
    loadOdt: loadOdt as never,
    save: (async (format = 'auto', opts: { images?: boolean; title?: string } = {}) => {
      const embed = opts.images !== false;
      const fmt = format === 'auto' ? c.loaded : format;
      const el = c.el as unknown as { exportPdf(): Promise<Uint8Array>; saveDocxWithFonts(): Promise<Uint8Array> };
      switch (fmt) {
        case 'odt': return exportOdt({ images: embed ? 'embed' : 'omit' });
        case 'pdf': return el.exportPdf();
        case 'html': return exportText(false, opts.title, { images: embed ? 'data' : 'omit' });
        case 'markdown': return exportText(true, opts.title, { images: embed ? 'data' : 'omit' });
        case 'xps': return exportXps();
        default: return el.saveDocxWithFonts();
      }
    }) as never,
    exportOdt: exportOdt as never,
    exportHtml: ((title?: string, opts?: { images?: 'data' | 'omit' }) => exportText(false, title, opts)) as never,
    exportMarkdown: ((title?: string, opts?: { images?: 'data' | 'omit' }) => exportText(true, title, opts)) as never,
    exportXps: exportXps as never,
    // ── imprimir: o código TS do elemento (setView, pageImageForPrint sobre a pintura roteada com printRender) ──
    print: ((...a: unknown[]) => c.ts('print', a)) as never,
    printPages: ((...a: unknown[]) => c.ts('printPages', a)) as never,
    /**
     * every image of the layout decoded (the TS awaits the decode promises before printing, 22924): each image asked once
     * (starts its decode), then the wait lasts while one is still LOADING — a failed decode (an undecodable picture) is
     * settled, as the TS placeholder; 30 s bound only against a decode that never ends
     */
    ensureImagesDecoded: (async () => {
      const dl = c.dl, api = c.api;
      const env = api?.paintEnv();
      if (!dl || !env || !api) return;
      // every page decoded first (fix round 2): the images table fills as the pages decode, and print paints them all
      for (const p of dl.pages) void p.items;
      const ids = dl.tables.images.map((_, i) => i);
      for (const i of ids) env.image(i);
      const deadline = performance.now() + 30000;
      while (c.dl === dl && performance.now() < deadline && ids.some((i) => api.imageState(i) === 'loading')) {
        await new Promise((r) => setTimeout(r, 16));
      }
    }) as never,
  };
}
