// B5b deck builder (group `b5b`): slides whose pictures are the synthetic metafiles, PICTs and TIFFs of meta-cases.mjs (EMF, EMF+, WMF,
// PICT, TIFF), at several sizes and crops (the viewer rasterizes a metafile at the size it has on the slide), plus the same media as a
// shape fill (no display size) and PNG pictures next to them. Built on the B1 package (`buildDeck` with this slide builder).
import { genEmf, genEmfPlus } from './meta-emf.mjs';
import { genWmf } from './meta-wmf.mjs';
import { genPict } from './meta-pict.mjs';
import { genTiff } from './meta-tiff.mjs';
import { NS, XML } from './parts.mjs';
import { randomPng } from '../png.mjs';
import { spXml } from './shapes.mjs';

const GRP = '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>';

const KINDS = [
  ['emf', 'emf', genEmf], ['emf', 'emf', genEmf], ['emfplus', 'emf', genEmfPlus], ['wmf', 'wmf', genWmf], ['wmf', 'wmf', genWmf],
  ['pict', 'pict', genPict], ['tiff', 'tif', genTiff], ['png', 'png', null],
];

export function b5bSlide(c, t, spec, profile) {
  const r = c.r;
  const tops = [];
  const n = r.int(profile.shapes[0], profile.shapes[1]);
  for (let i = 0; i < n; i++) {
    const [kind, ext, gen] = r.pick(KINDS);
    const bytes = gen ? gen(r.below(1e6)) : randomPng(r);
    const rId = c.addImage(bytes, ext);
    c.feat(`picture:${kind}`);
    const id = c.nextId();
    const w = Math.round(t.w * r.pick([0.1, 0.25, 0.4, 0.6, 0.9]));
    const h = Math.round(t.h * r.pick([0.1, 0.25, 0.4, 0.6, 0.9]));
    const x = r.int(0, Math.max(0, t.w - w)), y = r.int(0, Math.max(0, t.h - h));
    const src = r.chance(0.3) ? `<a:srcRect l="${r.int(0, 30000)}" t="${r.int(0, 30000)}" r="${r.int(0, 30000)}" b="${r.int(0, 30000)}"/>` : '';
    tops.push(`<p:pic><p:nvPicPr><p:cNvPr id="${id}" name="Picture ${id}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="${rId}"/>${src}<a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="${x}" y="${y}"/><a:ext cx="${w}" cy="${h}"/></a:xfrm><a:prstGeom prst="${r.pick(['rect', 'rect', 'rect', 'ellipse'])}"><a:avLst/></a:prstGeom></p:spPr></p:pic>`);
    if (r.chance(0.25)) {
      // the same media as the fill of a shape: no display size (the viewer's first use of the part decides)
      const id2 = c.nextId();
      tops.push(`<p:sp><p:nvSpPr><p:cNvPr id="${id2}" name="Fill ${id2}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="${r.below(t.w >> 1)}" y="${r.below(t.h >> 1)}"/><a:ext cx="${t.w >> 2}" cy="${t.h >> 2}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:blipFill><a:blip r:embed="${rId}"/><a:stretch><a:fillRect/></a:stretch></a:blipFill></p:spPr></p:sp>`);
    }
  }
  tops.push(spXml(c, { w: t.w, h: t.h }, { text: true }));
  return `${XML}<p:sld ${NS}><p:cSld><p:spTree>${GRP}${tops.join('')}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>`;
}
