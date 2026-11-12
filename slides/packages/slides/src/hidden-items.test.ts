// Itens ocultos no editor <universal-slides>: os slides ocultos continuam na
// lista (miniatura esmaecida, "Mostrar slide"), e forma oculta não é pintada
// nem selecionada pelo clique (o PowerPoint só a alcança pelo Painel de Seleção).
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';
import { installFakeDom, type FakeDom, type FakeNode } from '../../../../write/test-support/fake-dom.js';
import { buildPptx, sp, xfrm } from '../../slides-core/test/helpers/pptx-builder.js';
import type { UniversalSlidesElement as Slides } from './universal-slides.js';
import { hitShape } from './edit-surface.js';

let dom: FakeDom;
let Ctor: typeof Slides;

beforeAll(async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] });
  dom = installFakeDom();
  Ctor = (await import('./universal-slides.js')).UniversalSlidesElement;
}, 60_000);
afterAll(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const title = (t: string, attrs = ''): string =>
  sp(2, `<a:p><a:r><a:t>${t}</a:t></a:r></a:p>`, '<p:ph type="title"/>', xfrm(457200, 274638, 8229600, 1143000), attrs);

describe('editor e itens ocultos', () => {
  it('o editor mantém os slides ocultos', () => {
    const el = new Ctor();
    dom.document.body.appendChild(el as unknown as FakeNode);
    el.openPresentation(buildPptx({ slides: [{ tree: title('Um') }, { tree: title('Dois'), show: false }, { tree: title('Três') }] }).bytes());
    expect(el.getSlideCount()).toBe(3);
    expect([0, 1, 2].map((i) => el.isSlideHidden(i))).toEqual([false, true, false]);
    el.setSlideHidden(false, 1);
    expect(el.isSlideHidden(1)).toBe(false);
    expect(el.getSlideCount()).toBe(3);
    (el as unknown as FakeNode).remove();
  });

  it('o clique não seleciona forma oculta (seleciona a visível de baixo)', () => {
    const slide = buildPptx({ slides: [sp(3, '<a:p><a:r><a:t>baixo</a:t></a:r></a:p>', '', xfrm(0, 0, 1000000, 1000000))
      + sp(4, '<a:p><a:r><a:t>oculta</a:t></a:r></a:p>', '', xfrm(0, 0, 1000000, 1000000), ' hidden="1"')] }).slide();
    const fit = { ox: 0, oy: 0, scale: 1e-4 };
    expect(hitShape(slide, 50, 50, fit)).toBe(0);
  });
});
