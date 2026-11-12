import { describe, expect, it } from 'vitest';
import { paintSlideTo, hitLinkArea, type SlideLinkArea, type Slide } from '../src/index.js';

// Áreas clicáveis dos hiperlinks de TEXTO: coletadas durante a pintura (uma por
// pedaço de run com link, no retângulo local + a transformação do contexto),
// para o componente saber QUAL link está sob o ponteiro.

type M = { a: number; b: number; c: number; d: number; e: number; f: number };

/** Contexto que registra fillText e mantém a transformação (translate/scale/rotate/setTransform). */
function ctxWith(initial: M) {
  let m = { ...initial };
  const stack: M[] = [];
  const mul = (n: M): void => {
    m = { a: m.a * n.a + m.c * n.b, b: m.b * n.a + m.d * n.b, c: m.a * n.c + m.c * n.d, d: m.b * n.c + m.d * n.d, e: m.a * n.e + m.c * n.f + m.e, f: m.b * n.e + m.d * n.f + m.f };
  };
  const texts: string[] = [];
  const t: Record<string, unknown> = {
    texts,
    canvas: { width: 1000, height: 1000 },
    measureText: (s: string) => ({ width: s.length * 6 }),
    save: () => { stack.push({ ...m }); },
    restore: () => { m = stack.pop() ?? m; },
    getTransform: () => ({ ...m }),
    setTransform: (a: number, b: number, c: number, d: number, e: number, f: number) => { m = { a, b, c, d, e, f }; },
    translate: (x: number, y: number) => mul({ a: 1, b: 0, c: 0, d: 1, e: x, f: y }),
    scale: (x: number, y: number) => mul({ a: x, b: 0, c: 0, d: y, e: 0, f: 0 }),
    rotate: (r: number) => mul({ a: Math.cos(r), b: Math.sin(r), c: -Math.sin(r), d: Math.cos(r), e: 0, f: 0 }),
    fillText: (s: string) => { texts.push(s); },
  };
  return new Proxy(t, {
    get: (o, k: string) => (k in o ? o[k] : () => undefined),
    set: (o, k: string, v) => { o[k] = v; return true; },
  }) as unknown as CanvasRenderingContext2D & { texts: string[] };
}

const run = (text: string, hyperlink?: string, extra: object = {}) => ({ text, sz: 18, font: 'Arial', color: '#000000', ...(hyperlink ? { hyperlink } : {}), ...extra });
const RES = { slideW: 9144000, slideH: 5143500 };
const SCALE = 1 / 9525; // 1 px por pt·(96/72): o slide tem 960 × 540 px
const ID: M = { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 };

function linkSlide(fill = false): Slide {
  return {
    shapes: [{
      id: '3', name: 'links', xfrm: { x: 914400, y: 914400, cx: 6000000, cy: 3000000 },
      ...(fill ? { fill: { type: 'solid', color: '#FFFFFF' } } : {}),
      txBody: {
        paragraphs: [
          { runs: [run('primeiro', 'https://a.example/')] },
          { runs: [run('texto sem link ')] },
          { runs: [run('veja '), run('segundo', 'https://b.example/')] },
          { runs: [run('terceiro', 'https://c.example/')] },
        ],
      },
    }],
  } as unknown as Slide;
}

describe('áreas de link na pintura', () => {
  it('coleta uma área por run com link, com a transformação do contexto', () => {
    const dpr: M = { a: 2, b: 0, c: 0, d: 2, e: 0, f: 0 };
    const ctx = ctxWith(dpr);
    const areas: SlideLinkArea[] = [];
    paintSlideTo(ctx, linkSlide(), RES, SCALE, { ox: 10, oy: 20, linkAreas: areas });
    const links = areas.filter((a) => a.url);
    expect(links.map((a) => a.url)).toEqual(['https://a.example/', 'https://b.example/', 'https://c.example/']);
    for (const a of links) {
      expect(a.m).toEqual(dpr);
      expect(a.w).toBeGreaterThan(0);
      expect(a.h).toBeGreaterThan(0);
      // dentro da caixa da forma (px locais: ox + EMU × escala)
      expect(a.x).toBeGreaterThanOrEqual(10 + 914400 * SCALE - 1);
      expect(a.y).toBeGreaterThanOrEqual(20 + 914400 * SCALE - 1);
    }
    // um link por parágrafo, de cima para baixo; "segundo" começa depois de "veja "
    expect(links[0]!.y).toBeLessThan(links[1]!.y);
    expect(links[1]!.y).toBeLessThan(links[2]!.y);
    expect(links[1]!.x).toBeGreaterThan(links[0]!.x);
    // forma sem preenchimento não bloqueia nada
    expect(areas.some((a) => a.url === null)).toBe(false);
  });

  it('forma com preenchimento registra a própria caixa (url null) antes dos links dela', () => {
    const areas: SlideLinkArea[] = [];
    paintSlideTo(ctxWith(ID), linkSlide(true), RES, SCALE, { linkAreas: areas });
    expect(areas[0]!.url).toBeNull();
    expect(areas[0]).toMatchObject({ x: 914400 * SCALE, y: 914400 * SCALE, w: 6000000 * SCALE, h: 3000000 * SCALE });
    expect(areas.slice(1).map((a) => a.url)).toEqual(['https://a.example/', 'https://b.example/', 'https://c.example/']);
  });

  it('sem coletor nada é registrado (e o coletor de uma pintura não vaza para a seguinte)', () => {
    const areas: SlideLinkArea[] = [];
    paintSlideTo(ctxWith(ID), linkSlide(), RES, SCALE, { linkAreas: areas });
    const n = areas.length;
    expect(() => paintSlideTo(ctxWith(ID), linkSlide(), RES, SCALE)).not.toThrow();
    expect(areas).toHaveLength(n);
  });

  it('forma oculta não registra link', () => {
    const slide = linkSlide(true);
    slide.shapes[0]!.hidden = true;
    const areas: SlideLinkArea[] = [];
    paintSlideTo(ctxWith(ID), slide, RES, SCALE, { linkAreas: areas });
    expect(areas).toEqual([]);
  });

  it('run com preenchimento 100% transparente não vira área', () => {
    const slide = { shapes: [{ id: '2', name: 'x', xfrm: { x: 0, y: 0, cx: 6000000, cy: 1000000 }, txBody: { paragraphs: [{ runs: [run('invisível', 'https://x.example/', { alpha: 0 }), run(' visível', 'https://y.example/')] }] } }] } as unknown as Slide;
    const areas: SlideLinkArea[] = [];
    paintSlideTo(ctxWith(ID), slide, RES, SCALE, { linkAreas: areas });
    expect(areas.map((a) => a.url)).toEqual(['https://y.example/']);
  });

  it('forma girada: a área leva a rotação da forma', () => {
    const slide = { shapes: [{ id: '2', name: 'x', xfrm: { x: 0, y: 0, cx: 6000000, cy: 1000000, rot: 90 * 60000 }, txBody: { paragraphs: [{ runs: [run('link', 'https://x.example/')] }] } }] } as unknown as Slide;
    const areas: SlideLinkArea[] = [];
    paintSlideTo(ctxWith(ID), slide, RES, SCALE, { linkAreas: areas });
    const a = areas.find((x) => x.url)!;
    expect(a.m.a).toBeCloseTo(0, 6);
    expect(a.m.b).toBeCloseTo(1, 6);
    // o centro da área, levado ao canvas, cai no ponto que hitLinkArea acerta
    const cx = a.x + a.w / 2, cy = a.y + a.h / 2;
    const px = a.m.a * cx + a.m.c * cy + a.m.e, py = a.m.b * cx + a.m.d * cy + a.m.f;
    expect(hitLinkArea(areas, px, py)).toBe('https://x.example/');
  });
});

describe('hitLinkArea', () => {
  // retângulo local 100 × 10 girado 90° (horário no canvas) e deslocado 50 px
  const rot90: M = { a: 0, b: 1, c: -1, d: 0, e: 50, f: 0 };
  const area: SlideLinkArea = { url: 'https://u.example/', x: 0, y: 0, w: 100, h: 10, m: rot90 };

  it('acerta dentro de uma área girada (90°) e erra fora', () => {
    // local (50, 5) → canvas (−5 + 50, 50) = (45, 50)
    expect(hitLinkArea([area], 45, 50)).toBe('https://u.example/');
    // local (50, −10): acima do retângulo
    expect(hitLinkArea([area], 60, 50)).toBeNull();
    // local (150, 5): depois do fim
    expect(hitLinkArea([area], 45, 150)).toBeNull();
    expect(hitLinkArea([], 45, 50)).toBeNull();
  });

  it('a área de cima vence; uma forma opaca por cima (url null) bloqueia o link de baixo', () => {
    const under: SlideLinkArea = { url: 'https://baixo.example/', x: 0, y: 0, w: 100, h: 100, m: ID };
    const over: SlideLinkArea = { url: 'https://cima.example/', x: 0, y: 0, w: 50, h: 50, m: ID };
    const cover: SlideLinkArea = { url: null, x: 0, y: 0, w: 100, h: 100, m: ID };
    expect(hitLinkArea([under, over], 10, 10)).toBe('https://cima.example/');
    expect(hitLinkArea([under, over], 80, 80)).toBe('https://baixo.example/');
    expect(hitLinkArea([under, cover], 10, 10)).toBeNull();
    expect(hitLinkArea([cover, under], 10, 10)).toBe('https://baixo.example/');
  });

  it('matriz não invertível não acerta nada', () => {
    expect(hitLinkArea([{ ...area, m: { a: 0, b: 0, c: 0, d: 0, e: 0, f: 0 } }], 0, 0)).toBeNull();
  });
});
