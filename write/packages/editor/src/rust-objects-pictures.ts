// Frente O da leva 5 do engine="rust" (registro core/docs/modules/uo-bindings.md "Wave 5 — O: objects and the page"):
// IMAGENS e FORMAS/CAIXAS DE TEXTO como objetos — inserir, Formato da Imagem, Formato da Forma, organizar, Painel de
// Seleção, vínculo de caixas. Cada método é o do TS (universal-write.ts) sobre PictureCmd/DrawingCmd da sessão; o que
// depende do layout (o retângulo e as referências do objeto: `ObjFacts`) vem de `objects()`; os trabalhos de duas fases
// (decodificar/codificar) vão ao executor (rust-objects-jobs.ts).
import { uiLang } from '@universal-office/ui';
import { contentWidthTwips } from './rust-objects-page.js';
import {
  cmd, fromWire, locked, nullable, objects, omit, selectedHandle, selectedHandles, send, stateOf, wireColor, wireFormat, wireLine, wireShapeFill,
  type Handle, type ObjState,
} from './rust-objects-core.js';

type Dict = Record<string, unknown>;
type Dml = { kind: string; val: string; lastClr?: string; mods?: [string, number][] };


/** `{ rect, frame }` do objeto (o retângulo e as referências como o layout o colocou). */
export function factsOf(h: Handle | null): Dict {
  if (!h || h.inline) return {};
  return omit({ rect: { ...h.rect }, frame: h.frame ?? undefined });
}

/** O estado do objeto selecionado (`selectedObjectState`), `null` sem um. */
function objectState(o: ObjState, wantImage: boolean): Dict | null {
  const a = o.api?.session?.query({ t: 'selectedObjectState', ...(wantImage ? { wantImage } : {}) } as { t: string }) as { value?: Dict } | undefined;
  return a?.value ?? null;
}

/** O tamanho natural (pontos a 96 ppp) da imagem do objeto `h`: decodificada sob demanda, `null` enquanto carrega. */
export function naturalPt(o: ObjState, h: Handle | null): { w: number; h: number } | null {
  const b = bitmapOf(o, h);
  return b ? { w: b.width * 0.75, h: b.height * 0.75 } : null;
}

/** A imagem decodificada do objeto `h` (o `bitmaps` do TS); carrega em segundo plano e sincroniza as barras. */
export function bitmapOf(o: ObjState, h: Handle | null): ImageBitmap | null {
  if (!h || h.image === undefined) return null;
  const hit = o.bitmaps.get(h.id);
  if (hit && hit !== 'loading') return hit;
  if (hit === 'loading') return null;
  const bytes = o.api?.dl?.imageBytes(h.image);
  if (!bytes) return null;
  o.bitmaps.set(h.id, 'loading');
  void createImageBitmap(new Blob([new Uint8Array(bytes)])).then((b) => {
    o.bitmaps.set(h.id, b);
    (o.ed.el as unknown as { chromeSync: (() => void) | null }).chromeSync?.();
  }, () => { o.bitmaps.delete(h.id); });
  return null;
}

/** Espera a imagem do objeto `h` decodificar (o `naturalSizePt` do TS lê o bitmap já pintado). */
async function naturalPtAsync(o: ObjState, h: Handle | null): Promise<{ w: number; h: number } | null> {
  for (let i = 0; i < 50; i++) {
    const n = naturalPt(o, h);
    if (n || !h || o.bitmaps.get(h.id) !== 'loading') return n;
    await new Promise((r) => setTimeout(r, 10));
  }
  return null;
}

export function picturesHandlers(o: ObjState): Record<string, (...args: never[]) => unknown> {
  const pic = (v: Dict, what: string): boolean => (locked(o) ? false : cmd(o, { t: 'picture', v }, what));
  const picJobs = (v: Dict, what: string, pixels: boolean): Promise<unknown> => {
    if (locked(o)) return Promise.resolve(undefined);
    const r = send(o, { t: 'picture', v }, what, pixels);
    return Promise.resolve(r).then(() => undefined);
  };
  const draw = (v: Dict, what: string): boolean => (locked(o) ? false : cmd(o, { t: 'drawing', v }, what));
  const onShape = (v: Dict, what: string): boolean => draw({ t: 'onShape', v }, what);
  const sel = (): Handle | null => selectedHandle(o);
  const relTo = (r?: string): string => r ?? ((o.ed.el as unknown as { imageAlignTo?: string }).imageAlignTo ?? 'margin');

  /** `setImageWrap`: depois de passar a flutuar, o objeto fica onde estava (`PlaceAt` com as referências novas). */
  const setWrap = async (kind: string, shape: boolean): Promise<void> => {
    if (locked(o)) return;
    const before = sel();
    const v = { $t: 'setWrap', kind };
    const r = send(o, shape ? { t: 'drawing', v: { t: 'onShape', v } } : { t: 'picture', v }, shape ? 'setShapeWrap' : 'setImageWrap', true);
    await r;
    placeAfterFloat(before);
  };
  /** A colocação do objeto que acabou de flutuar (a sessão o deixa na origem até o `PlaceAt`; nada nos outros casos). */
  const placeAfterFloat = (before: Handle | null): void => {
    if (!before || !before.inline) return;
    const now = objects(o)?.floats.find((f) => f.id === before.id);
    if (!now?.frame) return;
    cmd(o, { t: 'drawing', v: { t: 'placeAt', frame: now.frame, x: before.rect.x, y: before.rect.y } }, 'placeAt');
  };
  const setLayout = async (opts: Dict, shape: boolean): Promise<void> => {
    if (locked(o)) return;
    const before = sel();
    const { distPt, relWidth, relHeight, ...rest } = opts as Dict & { distPt?: { t?: number; b?: number; l?: number; r?: number }; relWidth?: { relativeFrom: string; pct: number } | null; relHeight?: { relativeFrom: string; pct: number } | null };
    const rel = (x: { relativeFrom: string; pct: number } | null | undefined): Dict | undefined => (x === undefined ? undefined : x === null ? {} : { value: { f0: x.relativeFrom, f1: x.pct } });
    const o2 = omit({
      ...rest,
      distPt: distPt ? [distPt.t, distPt.b, distPt.l, distPt.r].map((x) => (x === undefined ? {} : { value: x })) : undefined,
      relWidth: rel(relWidth), relHeight: rel(relHeight),
    });
    const v = { $t: 'setLayout', o: o2, facts: factsOf(before) };
    await send(o, shape ? { t: 'drawing', v: { t: 'onShape', v } } : { t: 'picture', v }, shape ? 'setShapeLayout' : 'setImageLayout', true);
    // the dialog's own position wins over the placement (the TS: setImageWrap places, editDrawing writes posH/posV)
    if (opts['wrap'] !== undefined && opts['wrap'] !== 'inline' && opts['posH'] === undefined && opts['posV'] === undefined) placeAfterFloat(before);
  };
  const align = (kind: string, relativeTo: string | undefined, shape: boolean): void => {
    if (!relativeTo && selectedHandles(o).filter((x) => !x.inline).length > 1) { h['alignObjects']!(kind as never); return; }
    const v = omit({ $t: 'align', kind, relativeTo: relTo(relativeTo), rects: [factsOf(sel())] });
    if (shape) onShape(v, 'alignShape'); else pic(v, 'alignImage');
  };
  const imageState = (): Dict | null => {
    const s = objectState(o, true);
    if (!s) return null;
    const f = fromWire(s['format']) as Dict & { posV?: { relativeFrom?: string } };
    return {
      inline: !!s['inline'], wrap: s['wrap'],
      widthPt: (s['cx'] as number) / 12700, heightPt: (s['cy'] as number) / 12700,
      rotation: (s['rot'] as number) / 60000, flipH: !!s['flipH'], flipV: !!s['flipV'],
      lockAspect: !!s['lockAspect'],
      moveWithText: !!s['moveWithText'],
      lockAnchor: !!s['lockAnchor'],
      name: s['name'] ?? '', altText: s['altText'] ?? '', decorative: !!s['decorative'],
      naturalPt: naturalPt(o, sel()),
      format: f,
    };
  };
  const shapeState = (): Dict | null => {
    const s = objectState(o, false);
    if (!s || s['kind'] === 'image') return null;
    const out: Dict = {
      kind: s['kind'], inline: !!s['inline'], wrap: s['wrap'],
      widthPt: (s['cx'] as number) / 12700, heightPt: (s['cy'] as number) / 12700,
      rotation: (s['rot'] as number) / 60000, flipH: !!s['flipH'], flipV: !!s['flipV'],
      lockAspect: !!s['lockAspect'], moveWithText: !!s['moveWithText'], lockAnchor: !!s['lockAnchor'],
      name: s['name'] ?? '', altText: s['altText'] ?? '', decorative: !!s['decorative'],
      geometry: s['geometry'] ?? '', styleIndex: s['styleIndex'] ?? 0, editingText: !!s['editingText'],
      // the TS ShapeFormat has no `blip` nor `srcRect` (the wire's PictureFormat base always has them); `fill`, `body`, `story`
      // and `textbox` are not on the wire (ED-213)
      format: (({ blip: _b, srcRect: _s, ...rest }) => { void _b; void _s; return rest; })(fromWire(s['format']) as Dict),
    };
    const t = s['text'] as { marginsEmu: number[]; anchor: string; direction: string; wrap?: boolean; autofit: string; upright?: boolean } | undefined;
    if (t) {
      const [l, tt, r, b] = t.marginsEmu;
      out['text'] = { marginsPt: { l: l! / 12700, t: tt! / 12700, r: r! / 12700, b: b! / 12700 }, anchor: t.anchor, direction: t.direction, wrap: !!t.wrap, autofit: t.autofit, upright: !!t.upright };
    }
    if (s['kind'] === 'textbox') out['link'] = { next: !!s['linkNext'], previous: !!s['linkPrevious'] };
    return out;
  };

  const h: Record<string, (...args: any[]) => unknown> = {
    // ── inserir ──
    insertImage: async (data: Uint8Array, ext: string) => {
      if (locked(o)) return;
      const maxW = contentWidthTwips(o);
      await send(o, { t: 'picture', v: omit({ $t: 'insert', data: new Uint8Array(data), ext, maxWidthTwips: maxW }) }, 'insertImage', false);
    },
    insertTextbox: (id = 'simple') => !locked(o) && draw(omit({ t: 'insertTextbox', id, page: o.api.caret()?.page ?? 0, lang: uiLang(o.ed.el) }), 'insertTextbox'),
    // ── estado ──
    selectedImage: () => imageState(),
    selectedShape: () => shapeState(),
    selectedImagePreview: (w: number, hh: number): ImageData | null => {
      const s = objectState(o, true);
      const src = bitmapOf(o, sel());
      if (!s || !src) return null;
      const f = s['format'] as { srcRect: { l: number; t: number; r: number; b: number }; flipH?: boolean; flipV?: boolean };
      const sr = f.srcRect;
      const sx = Math.max(0, sr.l / 100000) * src.width, sy = Math.max(0, sr.t / 100000) * src.height;
      const sw = Math.max(1, (1 - (Math.max(0, sr.l) + Math.max(0, sr.r)) / 100000) * src.width);
      const sh = Math.max(1, (1 - (Math.max(0, sr.t) + Math.max(0, sr.b)) / 100000) * src.height);
      const k = Math.min(w / sw, hh / sh);
      const cw = Math.max(1, Math.round(sw * k)), ch = Math.max(1, Math.round(sh * k));
      const c = new OffscreenCanvas(cw, ch);
      const x = c.getContext('2d')!;
      x.translate(f.flipH ? cw : 0, f.flipV ? ch : 0);
      x.scale(f.flipH ? -1 : 1, f.flipV ? -1 : 1);
      x.drawImage(src, sx, sy, sw, sh, 0, 0, cw, ch);
      return x.getImageData(0, 0, cw, ch);
    },
    selectedObjectCount: () => (o.api?.session?.query({ t: 'selectedObjectCount' }).v as number | undefined) ?? 0,
    // ── Formato da Imagem ──
    deleteSelectedImage: () => pic({ $t: 'deleteSelectedImage' }, 'deleteSelectedImage'),
    setImageWrap: (kind: string) => setWrap(kind, false),
    setImagePosition: async (preset: string) => {
      if (preset === 'inline') { await setWrap('inline', false); return; }
      const before = sel();
      // (from in line: the session floats it at the origin and the preset's alignment replaces the position, as the TS)
      pic({ $t: 'setPosition', preset, facts: factsOf(before) }, 'setImagePosition');
    },
    setImageMoveWithText: (on: boolean) => pic(omit({ $t: 'setMoveWithText', on: on || undefined, facts: factsOf(sel()) }), 'setImageMoveWithText'),
    setImageLockAnchor: (on: boolean) => pic(omit({ $t: 'setLockAnchor', on: on || undefined }), 'setImageLockAnchor'),
    alignImage: (kind: string, relativeTo?: string) => align(kind, relativeTo, false),
    setImageSize: (opts: Dict) => pic({ $t: 'setSize', ...omit(opts) }, 'setImageSize'),
    setImageRotation: (deg: number) => pic({ $t: 'setRotation', deg }, 'setImageRotation'),
    rotateImage: (deg: number) => pic({ $t: 'rotate', deg }, 'rotateImage'),
    flipImage: (axis: 'h' | 'v') => pic(omit({ $t: 'flip', vertical: axis === 'v' || undefined }), 'flipImage'),
    setImageAltText: (opts: Dict) => pic({ $t: 'setAltText', ...omit(opts) }, 'setImageAltText'),
    arrangeImage: (op: string) => pic({ $t: 'arrange', op }, 'arrangeImage'),
    setImageLayout: (opts: Dict) => setLayout(opts, false),
    setImageBorder: (opts: { color?: Dml | null; weightPt?: number; dash?: string }) => pic(omit({ $t: 'setBorder', color: nullable(opts.color, wireColor), weightPt: opts.weightPt, dash: opts.dash }), 'setImageBorder'),
    setImageFormat: (opts: Dict) => pic({ $t: 'setFormat', fmt: wireFormat(opts) }, 'setImageFormat'),
    setImageEffect: (kind: string, preset: string | null) => pic(omit({ $t: 'setEffect', kind, preset: preset ?? undefined }), 'setImageEffect'),
    cropImageFillFit: async (mode: 'fill' | 'fit') => {
      const nat = await naturalPtAsync(o, sel());
      if (!nat) return;
      pic({ $t: 'cropFillFit', ...(mode === 'fill' ? { fill: true } : {}), naturalPt: { f0: nat.w, f1: nat.h } }, 'cropImageFillFit');
    },
    cropImageToAspect: (w: number, hh: number) => pic({ $t: 'cropToAspect', w, h: hh }, 'cropImageToAspect'),
    cropImageToShape: (prst: string) => pic({ $t: 'cropToShape', prst }, 'cropImageToShape'),
    setImageTransparency: (pct: number) => pic({ $t: 'setTransparency', pct }, 'setImageTransparency'),
    setImageRecolor: (preset: string | Dml) => pic({ $t: 'setRecolor', arg: typeof preset === 'string' ? { t: 'preset', v: preset } : { t: 'color', ...wireColor(preset) } }, 'setImageRecolor'),
    setImageTransparentColor: (hex: string | null) => pic(omit({ $t: 'setTransparentColor', hex: hex ?? undefined }), 'setImageTransparentColor'),
    setImageCrop: (c: { l: number; t: number; r: number; b: number }) => pic({ $t: 'setCrop', l: c.l, t: c.t, r: c.r, b: c.b }, 'setImageCrop'),
    resetImage: async (size = false) => {
      const nat = size ? await naturalPtAsync(o, sel()) : null;
      pic(omit({ $t: 'reset', size: size || undefined, naturalPt: nat ? { f0: nat.w, f1: nat.h } : undefined }), 'resetImage');
    },
    setImageAdjust: (opts: Dict) => picJobs({ $t: 'setAdjust', o: omit(opts) }, 'setImageAdjust', true),
    changeImage: (data: Uint8Array, ext: string) => {
      const before = sel();
      if (before) o.bitmaps.delete(before.id);
      return picJobs({ $t: 'change', data: new Uint8Array(data), ext }, 'changeImage', false);
    },
    changeImageFromFile: () => {
      const inp = document.createElement('input');
      inp.type = 'file';
      inp.accept = 'image/*';
      inp.style.display = 'none';
      inp.addEventListener('change', async () => {
        const file = inp.files?.[0];
        inp.remove();
        if (!file) return;
        const buf = new Uint8Array(await file.arrayBuffer());
        const ext = (file.name.split('.').pop() || 'png').toLowerCase();
        await (h['changeImage'] as (d: Uint8Array, e: string) => Promise<unknown>)(buf, ext);
      });
      document.body.appendChild(inp);
      inp.click();
    },
    compressImages: async (opts: { ppi: number; deleteCropped: boolean; all?: boolean }): Promise<number> => {
      if (locked(o)) return 0;
      await send(o, { t: 'picture', v: omit({ $t: 'compress', ppi: opts.ppi, deleteCropped: opts.deleteCropped || undefined, all: opts.all || undefined }) }, 'compressImages', true);
      o.bitmaps.clear();
      return (o.api?.session?.query({ t: 'compressResult' }).v as number | undefined) ?? 0;
    },
    // ── Formato da Forma ──
    setShapeFill: (fill: Dml | Dict | null) => {
      const isFill = (x: Dict): boolean => ['none', 'style', 'solid', 'gradient', 'pattern', 'picture', 'raw'].includes(x['kind'] as string);
      const f = fill === null ? { t: 'none' } : isFill(fill as Dict) ? wireShapeFill(fill as Dict) : { t: 'solid', ...wireColor(fill as Dml) };
      draw({ t: 'setShapeFill', fill: f }, 'setShapeFill');
    },
    setShapePictureFill: async (data: Uint8Array, ext: string, tile = false) => {
      if (locked(o)) return;
      await send(o, { t: 'drawing', v: omit({ t: 'setShapePictureFill', data: new Uint8Array(data), ext, tile: tile || undefined }) }, 'setShapePictureFill', false);
    },
    setShapeOutline: (opts: { color?: Dml | null; weightPt?: number; dash?: string; compound?: string }) => draw(omit({ t: 'setShapeOutline', color: nullable(opts.color, wireColor), weightPt: opts.weightPt, dash: opts.dash, compound: opts.compound }), 'setShapeOutline'),
    setShapeFormat: (opts: Dict) => draw({ t: 'setShapeFormat', fmt: wireFormat(opts) }, 'setShapeFormat'),
    setShapeStyle: (n: number) => draw({ t: 'setShapeStyle', n }, 'setShapeStyle'),
    changeShape: (prst: string) => draw({ t: 'changeShape', prst }, 'changeShape'),
    setTextboxOptions: (opts: { marginsPt?: { l?: number; t?: number; r?: number; b?: number }; anchor?: string; direction?: string; wrap?: boolean; autofit?: string; upright?: boolean }) => {
      const m = opts.marginsPt;
      draw({ t: 'setTextboxOptions', o: omit({ marginLPt: m?.l, marginTPt: m?.t, marginRPt: m?.r, marginBPt: m?.b, anchor: opts.anchor, direction: opts.direction, wrap: opts.wrap, autofit: opts.autofit, upright: opts.upright }) }, 'setTextboxOptions');
    },
    setShapeEffect: (kind: string, preset: string | null) => draw(omit({ t: 'setShapeEffect', kind, preset: preset ?? undefined }), 'setShapeEffect'),
    deleteSelectedShape: () => draw({ t: 'deleteSelectedShape' }, 'deleteSelectedShape'),
    setShapeWrap: (kind: string) => setWrap(kind, true),
    setShapePosition: (preset: string) => {
      if (preset === 'inline') return setWrap('inline', true);
      const before = sel();
      onShape({ $t: 'setPosition', preset, facts: factsOf(before) }, 'setShapePosition');
      return undefined;
    },
    setShapeMoveWithText: (on: boolean) => onShape(omit({ $t: 'setMoveWithText', on: on || undefined, facts: factsOf(sel()) }), 'setShapeMoveWithText'),
    setShapeLockAnchor: (on: boolean) => onShape(omit({ $t: 'setLockAnchor', on: on || undefined }), 'setShapeLockAnchor'),
    setShapeSize: (opts: Dict) => onShape({ $t: 'setSize', ...omit(opts) }, 'setShapeSize'),
    setShapeRotation: (deg: number) => onShape({ $t: 'setRotation', deg }, 'setShapeRotation'),
    setShapeAltText: (opts: Dict) => onShape({ $t: 'setAltText', ...omit(opts) }, 'setShapeAltText'),
    setShapeLayout: (opts: Dict) => setLayout(opts, true),
    alignShape: (kind: string, relativeTo?: string) => align(kind, relativeTo, true),
    arrangeShape: (op: string) => onShape({ $t: 'arrange', op }, 'arrangeShape'),
    flipShape: (axis: 'h' | 'v') => onShape(omit({ $t: 'flip', vertical: axis === 'v' || undefined }), 'flipShape'),
    rotateShape: (deg: number) => onShape({ $t: 'rotate', deg }, 'rotateShape'),
    alignObjects: (kind: string) => draw({ t: 'alignObjects', kind, rects: selectedHandles(o).map(factsOf) }, 'alignObjects'),
    setAsDefaultTextbox: () => draw({ t: 'setAsDefaultTextbox' }, 'setAsDefaultTextbox'),
    linkTextboxTo: (key: string) => draw({ t: 'linkTextboxTo', key }, 'linkTextboxTo'),
    breakTextboxLink: () => draw({ t: 'breakTextboxLink' }, 'breakTextboxLink'),
    // ── Painel de Seleção ──
    selectionPaneItems: () => ((o.api?.session?.query({ t: 'selectionPaneItems' }).v ?? []) as { key: string; name: string; hidden?: boolean; selected?: boolean; kind: string }[])
      .map((x) => ({ key: x.key, name: x.name, hidden: !!x.hidden, selected: !!x.selected, kind: x.kind })),
    selectPaneItem: (key: string, add = false) => { o.crop = null; draw(omit({ t: 'selectPaneItem', key, add: add || undefined }), 'selectPaneItem'); },
    selectPaneItems: (keys: string[]) => draw({ t: 'selectPaneItems', keys }, 'selectPaneItems'),
    renamePaneItem: (key: string, name: string) => draw({ t: 'renamePaneItem', key, name }, 'renamePaneItem'),
    setPaneItemHidden: (key: string | null, hidden: boolean) => draw(omit({ t: 'setPaneItemHidden', key: key ?? undefined, hidden: hidden || undefined }), 'setPaneItemHidden'),
  };
  void wireLine;
  return h as Record<string, (...args: never[]) => unknown>;
}

export { stateOf };
