// Frente O da leva 5 do engine="rust" (registro core/docs/modules/uo-bindings.md "Wave 5 — O: objects and the page"): o
// executor dos trabalhos de duas fases da sessão (`{ t: 'jobs' }`: decodificar e codificar imagens com os codecs do
// navegador, como uo-docx.md "Stays in the TS shell" pede). Cada trabalho é respondido por `WriteSession.complete`; o
// resultado de novo `jobs` (a 2ª rodada: as codificações depois das decodificações) roda outra vez; o fim vai ao
// `apply` da casca. Outros tipos (`readClipboard`, `renderClip`) respondem `failed`, salvo um tratador registrado em
// `jobKinds` (o encaixe da frente R). Declarado ED-205/ED-206.
import type { ExecOutcome } from '@universal-office/write-session';
import type { FrontApi } from './rust-engine.js';

/** O que um trabalho recebe (os campos gerados de core/schema/session.uos, `Job`). */
export interface JobIn { t: string; id: number; bytes?: Uint8Array; ext?: string; width?: number; height?: number; rgba?: Uint8Array; mime?: string; quality?: number }
/** A resposta (`JobResult`). */
export type JobOut = { t: 'image'; width: number; height: number; rgba?: Uint8Array } | { t: 'bytes'; v: Uint8Array } | { t: 'failed' } | { t: string; [k: string]: unknown };

/** Tratadores de outros tipos de trabalho (a frente R registra `readClipboard`/`renderClip` aqui). */
export const jobKinds = new Map<string, (job: JobIn) => Promise<JobOut>>();

/** Os executores ativos (a entrada de ponteiro e teclado da frente O espera; ED-205). */
const active = new WeakSet<FrontApi>();
export function jobsBusy(api: FrontApi): boolean { return active.has(api); }

const mimeOf = (ext: string): string => `image/${ext === 'jpg' ? 'jpeg' : ext === 'svg' ? 'svg+xml' : ext}`;

/** `decodePicture` do TS: createImageBitmap + OffscreenCanvas; `pixels` = com o RGBA (getImageData). */
async function decode(job: JobIn, pixels: boolean): Promise<JobOut> {
  try {
    const bmp = await createImageBitmap(new Blob([new Uint8Array(job.bytes ?? new Uint8Array())], { type: mimeOf((job.ext ?? '').toLowerCase()) }));
    try {
      if (!pixels) return { t: 'image', width: bmp.width, height: bmp.height };
      const c = new OffscreenCanvas(Math.max(1, bmp.width), Math.max(1, bmp.height));
      const x = c.getContext('2d')!;
      x.drawImage(bmp, 0, 0);
      const id = x.getImageData(0, 0, c.width, c.height);
      return { t: 'image', width: id.width, height: id.height, rgba: new Uint8Array(id.data.buffer, id.data.byteOffset, id.data.byteLength) };
    } finally { bmp.close(); }
  } catch { return { t: 'failed' }; }
}

/** `encodePng` / o `convertToBlob` de `compressImages` do TS: putImageData + OffscreenCanvas.convertToBlob(mime, quality). */
async function encode(job: JobIn): Promise<JobOut> {
  try {
    const w = job.width ?? 0, h = job.height ?? 0;
    const rgba = job.rgba ?? new Uint8Array();
    if (w < 1 || h < 1 || rgba.length !== w * h * 4) return { t: 'failed' };
    const c = new OffscreenCanvas(w, h);
    c.getContext('2d')!.putImageData(new ImageData(Uint8ClampedArray.from(rgba), w, h), 0, 0);
    const mime = job.mime || 'image/png';
    const blob = await c.convertToBlob(job.quality !== undefined && mime === 'image/jpeg' ? { type: mime, quality: job.quality } : { type: mime });
    return { t: 'bytes', v: new Uint8Array(await blob.arrayBuffer()) };
  } catch { return { t: 'failed' }; }
}

/**
 * Responde os trabalhos de `first` até a sessão terminar (`done`/`error`) e aplica o fim na casca. `pixels`: as
 * decodificações levam o RGBA (os comandos que processam pixels: ajustes, compactar, contorno de Justo/Através); só o
 * tamanho para inserir e trocar a imagem (ED-206). Devolve o fim aplicado (null se a sessão sumiu).
 */
export async function runJobs(api: FrontApi, first: ExecOutcome, what: string, pixels: boolean): Promise<ExecOutcome | null> {
  let o: ExecOutcome | null = first;
  // the session that issued the jobs: a document loaded or created while a codec runs ends the run (its job ids mean
  // nothing to the new session; the old one is parked or freed with its pending command)
  const s0 = api.session;
  if (!s0) return null;
  active.add(api);
  try {
    while (o && o.t === 'jobs') {
      const jobs = (o as unknown as { jobs: JobIn[] }).jobs;
      let next: ExecOutcome | null = null;
      for (const job of jobs) {
        const r = job.t === 'decodeImage' ? await decode(job, pixels)
          : job.t === 'encodeImage' ? await encode(job)
            : jobKinds.has(job.t) ? await jobKinds.get(job.t)!(job).catch((): JobOut => ({ t: 'failed' }))
              : { t: 'failed' } as JobOut;
        if (api.session !== s0) return null;
        next = s0.complete(job.id, r as { t: string });
      }
      o = next;
    }
  } catch (err) {
    // a throwing `complete` (or handler) leaves no command pending: the session would answer every later command 'busy'
    if (api.session === s0) { try { api.apply(s0.cancelPending(), what); } catch { /* the session already dropped it */ } }
    api.warnOnce(`jobs:${what}`, `[universal-write] engine="rust": the two-phase command "${what}" failed and was cancelled`, err);
    return null;
  } finally { active.delete(api); }
  if (o && api.session === s0) api.apply(o, what);
  return o;
}
