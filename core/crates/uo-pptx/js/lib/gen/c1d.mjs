// The C-6 slide builder (`profile.slide` hook of b1.mjs, group `c1d`): decks whose slides hold generated charts. Each chart is
// one of the `uo-charts` generator's `c:chartSpace` charts (`crates/uo-charts/js/gen/charts.mjs`, `buildChart`): the chart part
// `ppt/charts/chartN.xml`, its pictures (`r:embed` of picture and texture fills, as chart-local relationships), and for the
// `alt` theme a `themeOverride` part (other colours and fonts than the deck's, the way PowerPoint writes a pasted chart). The
// chartex families (`cx:chartSpace`, wrapped in `mc:AlternateContent` by PowerPoint) are not read by the TS slides reader and are
// skipped. Slides carry 1-2 charts at random boxes, a random shape now and then (z-order and clipping), and no timing.
import { buildChart, FAMILIES, SEED_BASE } from '../../../../uo-charts/js/gen/charts.mjs';
import { NS, XML, relsXml } from './parts.mjs';
import { spXml } from './shapes.mjs';

const GRP = '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>';
const A = 'http://schemas.openxmlformats.org/drawingml/2006/main';
const C = 'http://schemas.openxmlformats.org/drawingml/2006/chart';
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';

const ALT = { dk1: '1F1F1F', lt1: 'F2F2F2', dk2: '2C4D75', lt2: 'EEECE1', accent1: 'C0504D', accent2: '9BBB59', accent3: '8064A2', accent4: '4BACC6', accent5: 'F79646', accent6: '2C4D75', hlink: '0000FF', folHlink: '800080' };

function themeOverrideXml() {
  const clr = Object.entries(ALT).map(([k, v]) => `<a:${k}><a:srgbClr val="${v}"/></a:${k}>`).join('');
  return `${XML}<a:themeOverride xmlns:a="${A}"><a:themeElements><a:clrScheme name="Alt">${clr}</a:clrScheme><a:fontScheme name="Alt"><a:majorFont><a:latin typeface="Cambria"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Arial"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme></a:themeElements></a:themeOverride>`;
}

/** A chart of the generator that the Slides reader reads (`c:chartSpace`), the `n`-th try of this slide. */
function pickChart(r, features) {
  for (let t = 0; t < 8; t++) {
    const i = r.int(0, 599);
    const ch = buildChart(SEED_BASE + i);
    if (ch.xml.includes('<cx:')) continue;
    features.add(`chart:${ch.name}`);
    return { ch, i };
  }
  const ch = buildChart(SEED_BASE);
  return { ch, i: 0 };
}

export function c1dSlide(c, t, spec, profile) {
  const r = c.r;
  const area = { w: t.w, h: t.h };
  const tops = [];
  const nCharts = r.int(1, 2);
  for (let k = 0; k < nCharts; k++) {
    const { ch } = pickChart(r, c.features);
    const n = c.media.filter(([name]) => /^ppt\/charts\/chart\d+\.xml$/.test(name)).length + 1;
    // the chart's pictures get deck-unique names; the chart keeps its own relationship ids
    const rels = [];
    for (const [rid, im] of Object.entries(ch.images)) {
      const ext = /\.([a-z0-9]+)$/i.exec(im.part)?.[1] ?? 'png';
      const name = `ppt/media/image${c.media.length + 1}.${ext}`;
      c.media.push([name, im.bytes]);
      rels.push([rid, 'image', `../${name.slice('ppt/'.length)}`]);
    }
    if (ch.themeName === 'alt') {
      const tn = `ppt/theme/themeOverride${n}.xml`;
      c.media.push([tn, themeOverrideXml()]);
      rels.push(['rId90', 'themeOverride', `../theme/themeOverride${n}.xml`]);
      c.feat('chart:themeOverride');
    }
    c.media.push([`ppt/charts/chart${n}.xml`, ch.xml]);
    if (rels.length) c.media.push([`ppt/charts/_rels/chart${n}.xml.rels`, relsXml(rels)]);
    const rid = c.addRel('chart', `../charts/chart${n}.xml`);
    const id = c.nextId();
    // 55-100 % of the slide, any aspect
    const cx = Math.floor(area.w * (0.55 + 0.45 * (r.int(0, 1000) / 1000)));
    const cy = Math.floor(area.h * (0.4 + 0.6 * (r.int(0, 1000) / 1000)));
    const x = r.int(0, Math.max(0, area.w - cx));
    const y = r.int(0, Math.max(0, area.h - cy));
    tops.push(`<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="${id}" name="Chart ${id}"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="${x}" y="${y}"/><a:ext cx="${cx}" cy="${cy}"/></p:xfrm><a:graphic><a:graphicData uri="${C}"><c:chart xmlns:c="${C}" xmlns:r="${R}" r:id="${rid}"/></a:graphicData></a:graphic></p:graphicFrame>`);
    c.feat('chart');
  }
  if (r.chance(0.4)) tops.push(spXml(c, area));
  // a shape over or under the chart: shuffle once
  if (tops.length > 1 && r.chance(0.5)) tops.reverse();
  return `${XML}<p:sld ${NS}><p:cSld><p:spTree>${GRP}${tops.join('')}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>`;
}

export { FAMILIES };
