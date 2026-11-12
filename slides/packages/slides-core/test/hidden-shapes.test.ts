import { describe, expect, it } from 'vitest';
import { buildPptx, sp, xfrm, REL } from './helpers/pptx-builder.js';

// Formas ocultas (Painel de Seleção do PowerPoint: `<p:cNvPr hidden="1">`) —
// o app não as desenha. A leitura MARCA (`hidden: true`) em vez de podar: o
// editor edita o XML pelo índice das formas e o export relê o mesmo modelo.

const XF = xfrm(0, 0, 1000000, 500000);
const txt = (t: string): string => `<a:p><a:r><a:t>${t}</a:t></a:r></a:p>`;
const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
const pic = (id: number, attrs: string): string =>
  `<p:pic><p:nvPicPr><p:cNvPr id="${id}" name="p${id}"${attrs}/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId5"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>${XF}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>`;
const cxn = (id: number, attrs: string): string =>
  `<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="${id}" name="c${id}"${attrs}/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr>${XF}<a:prstGeom prst="line"><a:avLst/></a:prstGeom><a:ln w="12700"><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:ln></p:spPr></p:cxnSp>`;
const table = (id: number, attrs: string): string =>
  `<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="${id}" name="t${id}"${attrs}/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="0" y="0"/><a:ext cx="1000000" cy="400000"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr/><a:tblGrid><a:gridCol w="1000000"/></a:tblGrid><a:tr h="400000"><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>x</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>`;
const group = (id: number, attrs: string, children: string): string =>
  `<p:grpSp><p:nvGrpSpPr><p:cNvPr id="${id}" name="g${id}"${attrs}/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="2000000" cy="1000000"/><a:chOff x="0" y="0"/><a:chExt cx="2000000" cy="1000000"/></a:xfrm></p:grpSpPr>${children}</p:grpSp>`;

function slideOf(tree: string, extra: Parameters<typeof buildPptx>[0] = { slides: [] }) {
  return buildPptx({
    ...extra,
    slides: [{ tree, rels: REL('rId5', 'image', '../media/image1.png') }],
    media: { 'ppt/media/image1.png': png },
  }).slide();
}

describe('formas ocultas (cNvPr hidden)', () => {
  it('hidden="1" e hidden="true" marcam sp, pic, cxnSp e graphicFrame', () => {
    for (const h of ['1', 'true']) {
      const a = ` hidden="${h}"`;
      const s = slideOf(sp(2, txt('a'), '', XF, a) + pic(3, a) + cxn(4, a) + table(5, a));
      expect(s.shapes.map((x) => [x.id, x.hidden])).toEqual([['2', true], ['3', true], ['4', true], ['5', true]]);
    }
  });

  it('hidden="0" ou ausente não marca', () => {
    const s = slideOf(sp(2, txt('a'), '', XF, ' hidden="0"') + pic(3, '') + cxn(4, ' hidden="false"') + table(5, ''));
    expect(s.shapes).toHaveLength(4);
    for (const x of s.shapes) expect(x.hidden).toBeUndefined();
  });

  it('grupo oculto marca todos os filhos (inclusive de grupo aninhado); grupo visível não', () => {
    const s = slideOf(
      group(10, ' hidden="1"', sp(11, txt('a'), '', XF) + group(12, '', sp(13, txt('b'), '', XF)))
      + group(20, '', sp(21, txt('c'), '', XF) + sp(22, txt('d'), '', XF, ' hidden="1"')),
    );
    expect(s.shapes.map((x) => [x.id, x.hidden ?? false])).toEqual([['11', true], ['13', true], ['21', false], ['22', true]]);
  });

  it('forma oculta dentro de mc:AlternateContent é marcada', () => {
    const alt = `<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice Requires="x"/><mc:Fallback>${sp(7, txt('a'), '', XF, ' hidden="1"')}</mc:Fallback></mc:AlternateContent>`;
    const s = slideOf(alt + sp(8, txt('b'), '', XF));
    expect(s.shapes.map((x) => [x.id, x.hidden ?? false])).toEqual([['7', true], ['8', false]]);
  });

  it('layout: forma oculta fica fora das decorações, mas o placeholder oculto continua dando a geometria', () => {
    const layout = sp(2, txt('decoração oculta'), '', xfrm(10, 10, 100, 100), ' hidden="1"')
      + sp(3, txt('decoração'), '', xfrm(20, 20, 100, 100))
      + sp(4, '', '<p:ph type="body" idx="1"/>', xfrm(111, 222, 333, 444), ' hidden="1"');
    const s = slideOf(sp(9, txt('corpo'), '<p:ph type="body" idx="1"/>'), { slides: [], layout });
    const inherited = s.shapes.filter((x) => x.inherited);
    expect(inherited.map((x) => x.id)).toEqual(['3']);
    const body = s.shapes.find((x) => x.id === '9')!;
    expect(body.xfrm).toMatchObject({ x: 111, y: 222, cx: 333, cy: 444 });
    expect(body.hidden).toBeUndefined();
  });
});
