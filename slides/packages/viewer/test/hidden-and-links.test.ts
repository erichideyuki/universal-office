// Bug 13707 (atividade Apresentação do clickCompliance): itens ocultos do
// PowerPoint apareciam e, num slide com vários links, qualquer clique abria o
// primeiro. Elemento de verdade sobre o DOM falso do Write; o .pptx é montado
// no teste.
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { installFakeDom, mouseEvent, type FakeDom, type FakeNode } from '../../../../write/test-support/fake-dom.js';
import { buildPptx, sp, xfrm, hlinkRel } from '../../slides-core/test/helpers/pptx-builder.js';
import type { UniversalSlidesViewerElement as Viewer } from '../src/universal-slides-viewer.js';

let dom: FakeDom;
let Ctor: typeof Viewer;

beforeAll(async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] });
  dom = installFakeDom();
  Ctor = (await import('../src/universal-slides-viewer.js')).UniversalSlidesViewerElement;
}, 60_000);
afterAll(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
beforeEach(() => { dom.opened.length = 0; });

interface Area { url: string | null; x: number; y: number; w: number; h: number }
interface Inner { canvas: HTMLCanvasElement; linkAreas: Area[] }
const inner = (el: Viewer): Inner => el as unknown as Inner;

/** Viewer montado com o canvas em 960 × 540 (o DOM falso não tem layout). */
function mount(bytes: Uint8Array): Viewer {
  const el = new Ctor();
  el.setAttribute('lang', 'en');
  dom.document.body.appendChild(el as unknown as FakeNode);
  Object.assign(inner(el).canvas, { clientWidth: 960, clientHeight: 540 });
  el.loadPptx(bytes);
  vi.advanceTimersByTime(20);
  return el;
}

const title = (id: number, t: string, attrs = ''): string =>
  sp(id, `<a:p><a:r><a:t>${t}</a:t></a:r></a:p>`, '<p:ph type="title"/>', xfrm(457200, 274638, 8229600, 1143000), attrs);
const label = (el: Viewer): string => inner(el).canvas.getAttribute('aria-label') ?? '';

describe('slides ocultos (show="0")', () => {
  const three = (hidden: boolean[]): Uint8Array => buildPptx({
    slides: ['Um', 'Dois', 'Três'].map((t, i) => ({ tree: title(2, t), show: !hidden[i] })),
  }).bytes();

  it('ficam fora da contagem, da navegação e do us-loaded', () => {
    const el = new Ctor();
    const counts: number[] = [];
    el.addEventListener('us-loaded', (e) => counts.push((e as CustomEvent<{ count: number }>).detail.count));
    dom.document.body.appendChild(el as unknown as FakeNode);
    Object.assign(inner(el).canvas, { clientWidth: 960, clientHeight: 540 });
    el.setAttribute('lang', 'en');
    el.loadPptx(three([false, true, false]));
    expect(counts).toEqual([2]);
    expect(el.getSlideCount()).toBe(2);
    expect(label(el)).toBe('Slide 1 of 2: Um');
    el.next();
    expect(el.getSlideIndex()).toBe(1);
    expect(label(el)).toBe('Slide 2 of 2: Três');
    el.next();
    expect(el.getSlideIndex()).toBe(1);
    (el as unknown as FakeNode).remove();
  });

  it('todos ocultos: mostra todos', () => {
    const el = mount(three([true, true, true]));
    expect(el.getSlideCount()).toBe(3);
    (el as unknown as FakeNode).remove();
  });

  it('a faixa de slides da apresentação (sldRg) vale pela posição original no arquivo', () => {
    // 4 slides, o 2º oculto; "Configurar apresentação": do 3º ao 4º
    const bytes = buildPptx({
      slides: ['Um', 'Dois', 'Três', 'Quatro'].map((t, i) => ({ tree: title(2, t), show: i !== 1 })),
      media: { 'ppt/presProps.xml': new TextEncoder().encode('<?xml version="1.0"?><p:presentationPr xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:showPr><p:sldRg st="3" end="4"/></p:showPr></p:presentationPr>') },
    }).bytes();
    const el = mount(bytes);
    expect(el.getSlideCount()).toBe(3);
    el.present('start', { fullscreen: false });
    expect(el.getSlideIndex()).toBe(1); // o "Três"
    el.stopPresentation();
    (el as unknown as FakeNode).remove();
  });
});

describe('formas ocultas', () => {
  it('a camada acessível não lista forma oculta (nem como título)', () => {
    const tree = sp(2, '<a:p><a:r><a:t>Texto Oculto</a:t></a:r></a:p>', '<p:ph type="ctrTitle"/>', xfrm(0, 0, 4000000, 1000000), ' hidden="1"')
      + sp(3, '<a:p><a:r><a:t>Texto não oculto</a:t></a:r></a:p>', '<p:ph type="subTitle" idx="1"/>', xfrm(0, 1500000, 4000000, 1000000));
    const el = mount(buildPptx({ slides: [tree] }).bytes());
    const root = (el as unknown as { shadowRoot: ShadowRoot }).shadowRoot;
    const content = [...root.querySelectorAll<HTMLElement>('*')].find((e) => e.getAttribute?.('part') === 'slide-text')!;
    expect(content.textContent).toContain('Texto não oculto');
    expect(content.textContent).not.toContain('Oculto');
    expect(label(el)).not.toContain('Oculto');
    (el as unknown as FakeNode).remove();
  });
});

describe('vários links no mesmo slide', () => {
  const URLS = ['https://a.example/', 'https://b.example/', 'https://c.example/'];
  const BOX = xfrm(914400, 914400, 7315200, 3200400);
  const linkPara = (rid: string, t: string): string => `<a:p><a:r><a:rPr lang="pt-BR"><a:hlinkClick r:id="${rid}"/></a:rPr><a:t>${t}</a:t></a:r></a:p>`;
  const body = linkPara('rIdA', URLS[0]!) + '<a:p><a:r><a:t>sem link</a:t></a:r></a:p>' + linkPara('rIdB', URLS[1]!) + linkPara('rIdC', URLS[2]!);
  const rels = hlinkRel('rIdA', URLS[0]!) + hlinkRel('rIdB', URLS[1]!) + hlinkRel('rIdC', URLS[2]!);
  const center = (a: Area): { clientX: number; clientY: number } => ({ clientX: a.x + a.w / 2, clientY: a.y + a.h / 2 });

  it('cada link de texto abre o próprio endereço; texto sem link não abre', () => {
    const el = mount(buildPptx({ slides: [{ tree: sp(2, body, '', BOX), rels }] }).bytes());
    const { canvas, linkAreas } = inner(el);
    const areas = linkAreas.filter((a) => a.url);
    expect(areas.map((a) => a.url)).toEqual(URLS);
    for (const a of areas) canvas.dispatchEvent(mouseEvent('click', center(a)));
    expect(dom.opened.map((o) => o[0])).toEqual(URLS);
    expect(dom.opened.every((o) => o[1] === '_blank' && /noopener/.test(o[2] ?? '') && /noreferrer/.test(o[2] ?? ''))).toBe(true);
    // dentro da caixa da forma, longe de qualquer link (canto inferior direito)
    dom.opened.length = 0;
    canvas.dispatchEvent(mouseEvent('click', { clientX: 96 + 768 - 10, clientY: 96 + 336 - 10 }));
    expect(dom.opened).toEqual([]);
    (el as unknown as FakeNode).remove();
  });

  it('cursor de mão só sobre o link', () => {
    const el = mount(buildPptx({ slides: [{ tree: sp(2, body, '', BOX), rels }] }).bytes());
    const { canvas, linkAreas } = inner(el);
    canvas.dispatchEvent(mouseEvent('mousemove', center(linkAreas.find((a) => a.url)!)));
    expect(canvas.style.cursor).toBe('pointer');
    canvas.dispatchEvent(mouseEvent('mousemove', { clientX: 96 + 768 - 10, clientY: 96 + 336 - 10 }));
    expect(canvas.style.cursor).toBe('default');
    (el as unknown as FakeNode).remove();
  });

  it('link escondido atrás de forma com preenchimento não abre', () => {
    const cover = `<p:sp><p:nvSpPr><p:cNvPr id="9" name="capa"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>${BOX}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></p:spPr></p:sp>`;
    const el = mount(buildPptx({ slides: [{ tree: sp(2, body, '', BOX) + cover, rels }] }).bytes());
    const { canvas, linkAreas } = inner(el);
    const first = linkAreas.find((a) => a.url)!;
    canvas.dispatchEvent(mouseEvent('click', center(first)));
    expect(dom.opened).toEqual([]);
    canvas.dispatchEvent(mouseEvent('mousemove', center(first)));
    expect(canvas.style.cursor).toBe('default');
    (el as unknown as FakeNode).remove();
  });

  it('link em forma oculta não abre', () => {
    const el = mount(buildPptx({ slides: [{ tree: sp(2, body, '', BOX, ' hidden="1"'), rels }] }).bytes());
    expect(inner(el).linkAreas).toEqual([]);
    inner(el).canvas.dispatchEvent(mouseEvent('click', { clientX: 120, clientY: 110 }));
    expect(dom.opened).toEqual([]);
    (el as unknown as FakeNode).remove();
  });
});
