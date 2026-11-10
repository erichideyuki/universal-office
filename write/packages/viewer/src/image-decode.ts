/** px CSS por twip (96 dpi). */
export const PX_PER_TWIP = 96 / 1440;

/** O que a decodificação lê de uma imagem do documento (o `InlineImage` do motor TS e a entrada da tabela do motor Rust). */
export interface ImageSource {
  data: Uint8Array;
  ext: string;
  /** a versão vetorial (SVG), pintada no lugar do raster quando houver */
  svg?: Uint8Array | undefined;
  widthTwips?: number | undefined;
  heightTwips?: number | undefined;
}

/**
 * Decodifica a imagem (o vetor SVG, como o Word, quando houver) e chama `done` com o bitmap (ou null).
 * Assíncrona: o chamador guarda o estado "carregando". Uma só função para os dois motores (TS e Rust).
 */
export function decodeImage(img: ImageSource, done: (b: ImageBitmap | HTMLCanvasElement | null) => void): void {
  const svg = img.svg;
  if (svg) {
    const url = URL.createObjectURL(new Blob([new Uint8Array(svg)], { type: 'image/svg+xml' }));
    const el = new Image();
    el.onload = () => {
      try {
        const SS = 3;
        const wt = img.widthTwips, ht = img.heightTwips;
        const oc = document.createElement('canvas');
        oc.width = Math.min(4096, Math.max(1, Math.round((wt ?? el.width / PX_PER_TWIP) * PX_PER_TWIP * SS)));
        oc.height = Math.min(4096, Math.max(1, Math.round((ht ?? el.height / PX_PER_TWIP) * PX_PER_TWIP * SS)));
        oc.getContext('2d')!.drawImage(el, 0, 0, oc.width, oc.height);
        done(oc);
      } catch { done(null); }
      URL.revokeObjectURL(url);
    };
    el.onerror = () => { URL.revokeObjectURL(url); done(null); };
    el.src = url;
    return;
  }
  const ext = img.ext === 'jpg' ? 'jpeg' : img.ext;
  void createImageBitmap(new Blob([new Uint8Array(img.data)], { type: `image/${ext}` })).then(done, () => done(null));
}
