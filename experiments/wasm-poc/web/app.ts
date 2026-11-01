// POC no navegador: o motor .NET (UniversalOffice.Docx) em WebAssembly diagrama o .docx e o
// pintor de canvas do motor TS (docx-layout/src/canvas.ts) desenha a árvore que veio do C#.
// Clicar numa página faz o hit-test no C# (geometria das linhas do motor) e mostra o caret.
import { paintItems, paintPageBackground, SfntFont, type EnginePaintEnv } from '../../../write/packages/docx-layout/src/index.ts';
import { FONT_FILES } from '../fonts.mjs';

type Engine = {
  AddFont(family: string, bold: boolean, italic: boolean, bytes: Uint8Array): void;
  Load(docx: Uint8Array): string;
  Relayout(): number;
  LayoutJson(): string;
  FaceBytes(id: number): Uint8Array;
  ImageBytes(id: number): Uint8Array;
  HitTest(page: number, x: number, y: number): string;
  ToPdf(): Uint8Array;
};

const log = document.getElementById('log')!;
const pagesEl = document.getElementById('pages')!;
const timings: [string, string][] = [];
const r = (v: number): string => `${Math.round(v * 10) / 10} ms`;
const note = (k: string, v: string): void => {
  timings.push([k, v]);
  log.innerHTML = timings.map(([a, b]) => `<tr><td>${a}</td><td>${b}</td></tr>`).join('');
};
const time = async <T>(label: string, f: () => T | Promise<T>): Promise<T> => {
  const t = performance.now();
  const v = await f();
  note(label, r(performance.now() - t));
  return v;
};

const tAll = performance.now();
const engine = await time('Partida do runtime .NET (dotnet.js + .wasm)', async () => {
  const { dotnet } = await import(/* @vite-ignore */ new URL('./wasm/_framework/dotnet.js', location.href).href);
  const runtime = await dotnet.create();
  return (await runtime.getAssemblyExports('WasmPoc.dll')).WasmPoc.Engine as Engine;
});

const fontBytes = await time('Download das 20 fontes', () => Promise.all(FONT_FILES.map(async (f) =>
  new Uint8Array(await (await fetch(`fonts/${f.path}`)).arrayBuffer()))));
await time('Fontes → C# (AddFont)', () => FONT_FILES.forEach((f, i) => engine.AddFont(f.family, f.bold, f.italic, fontBytes[i]!)));
const docx = new Uint8Array(await (await fetch('demo.docx')).arrayBuffer());

const load = JSON.parse(engine.Load(docx)) as { readMs: number; layoutMs: number; pages: number };
note('C#: leitura do .docx (frio)', r(load.readMs));
note('C#: layout (frio, 1ª vez)', r(load.layoutMs));

const json = await time('C#: árvore → JSON', () => engine.LayoutJson());
note('Tamanho do JSON', `${Math.round(json.length / 1024)} KB`);
const tree = await time('JS: JSON.parse', () => JSON.parse(json));

// faces: cada arquivo vira uma família CSS própria (o pintor faz negrito/itálico sintéticos)
const faces = await time('Faces: FontFace CSS + SfntFont (pintura)', async () => Promise.all(tree.faces.map(async (f: Record<string, unknown>, id: number) => {
  const bytes = engine.FaceBytes(id);
  const css = new FontFace(`uof${id}`, bytes);
  await css.load();
  document.fonts.add(css);
  return { ...f, id, font: new SfntFont(bytes) };
})));
const images = await time('Imagens: decodificação', async () => Promise.all(tree.images.map(async (im: { ext: string }, id: number) => {
  const data = engine.ImageBytes(id);
  const type = im.ext === 'jpg' || im.ext === 'jpeg' ? 'image/jpeg' : `image/${im.ext}`;
  let bitmap: ImageBitmap | null = null;
  try { bitmap = await createImageBitmap(new Blob([data as BlobPart], { type })); } catch { /* formato sem decodificador (emf/wmf) */ }
  return { ...im, data, bitmap };
})));
for (const p of tree.pages) {
  for (const it of p.items) {
    if (typeof it.face === 'number') it.face = faces[it.face];
    if (typeof it.image === 'number') it.image = images[it.image];
  }
}
const env: EnginePaintEnv = {
  fontOf: (face) => ({ family: `"uof${(face as unknown as { id: number }).id}", sans-serif`, weight: 400, italic: false }),
  image: (img) => (img as unknown as { bitmap: ImageBitmap | null }).bitmap,
  color: (hex) => `#${hex}`,
};

const dpr = window.devicePixelRatio || 1;
const cssW = Math.min(820, pagesEl.clientWidth || 820);
const canvases: HTMLCanvasElement[] = [];
await time(`Pintura das ${tree.pages.length} páginas (pintor TS)`, () => {
  tree.pages.forEach((p: { widthTw: number; heightTw: number; items: never[] }, i: number) => {
    const c = document.createElement('canvas');
    const s = cssW / p.widthTw;
    c.style.width = `${cssW}px`;
    c.style.height = `${p.heightTw * s}px`;
    c.width = Math.round(cssW * dpr);
    c.height = Math.round(p.heightTw * s * dpr);
    const ctx = c.getContext('2d')!;
    ctx.setTransform(s * dpr, 0, 0, s * dpr, 0, 0);
    paintPageBackground(ctx, p as never, env, { paper: '#ffffff', devicePxTw: 1 / (s * dpr), screen: true });
    paintItems(ctx, p.items, env);
    c.dataset['page'] = String(i);
    c.addEventListener('click', (e) => {
      const b = c.getBoundingClientRect();
      const x = (e.clientX - b.left) / s, y = (e.clientY - b.top) / s;
      const t = performance.now();
      const hit = JSON.parse(engine.HitTest(i, x, y));
      const ms = performance.now() - t;
      document.getElementById('hit')!.textContent = hit ? `hit-test no C#: ${hit.ref} offset ${hit.offset} em ${ms.toFixed(2)} ms` : 'sem linha';
      const caret = document.getElementById('caret')!;
      if (!hit) { caret.hidden = true; return; }
      caret.hidden = false;
      caret.style.left = `${b.left + window.scrollX + hit.x * s}px`;
      caret.style.top = `${b.top + window.scrollY + hit.y * s}px`;
      caret.style.height = `${hit.height * s}px`;
    });
    pagesEl.appendChild(c);
    canvases.push(c);
  });
});
note('Total até a 1ª pintura', r(performance.now() - tAll));
// depois da pintura: o layout de novo, já aquecido (o custo de cada reformatação)
await new Promise((res) => setTimeout(res, 50));
const warm: number[] = [];
for (let i = 0; i < 5; i++) warm.push(engine.Relayout());
note('C#: layout quente (melhor de 5)', `${r(Math.min(...warm))} (${warm.map((w) => Math.round(w)).join(', ')})`);

document.getElementById('pdf')!.addEventListener('click', () => {
  const t = performance.now();
  const pdf = engine.ToPdf();
  note('C#: PDF', `${r(performance.now() - t)} (${Math.round(pdf.length / 1024)} KB)`);
  window.open(URL.createObjectURL(new Blob([pdf as BlobPart], { type: 'application/pdf' })));
});
(window as unknown as { poc: unknown }).poc = { engine, tree, timings };
