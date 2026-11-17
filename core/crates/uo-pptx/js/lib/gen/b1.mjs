// The B1 deck builder: one seeded .pptx per call (slides with shapes, text, tables, pictures,
// groups, placeholders over layouts and a master, animations, transitions, notes).
// Later groups (B3 layout/text, B4 render, B5 SmartArt/metafiles, B6 timing) add profiles or
// builders next to this one and register them in groups.mjs.
import { zipBytes } from '../zip.mjs';
import { Rand } from './rand.mjs';
import { NS, XML, appXml, contentTypes, coreXml, layoutSpecs, layoutXml, makeTemplate, masterXml, notesMasterXml, notesSlideXml, presPropsXml, presentationXml, relsXml, tableStylesXml, themeXml } from './parts.mjs';
import { cxnXml, grpXml, phXml, picXml, spXml, tableXml } from './shapes.mjs';
import { fillXml } from './dml.mjs';
import { timingXml, transitionXml } from './anim.mjs';
import { words } from './text.mjs';

const GRP = '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>';

/** Collects the media of the package and the relationships of one part. */
class PartCtx {
  constructor(r, features, media) {
    this.r = r;
    this.features = features;
    this.media = media;
    this.rels = [];
    this.id = 1;
  }
  feat(tag) { this.features.add(tag); }
  nextId() { return ++this.id; }
  #rid() { return `rId${this.rels.length + 2}`; } // rId1 is the layout / master / theme link
  addImage(png, ext = 'png') {
    const name = `image${this.media.length + 1}.${ext}`;
    this.media.push([`ppt/media/${name}`, png]);
    const id = this.#rid();
    this.rels.push([id, 'image', `../media/${name}`]);
    return id;
  }
  /** A relationship of any type to a package-internal target (B6: media). */
  addRel(type, target) {
    const id = this.#rid();
    this.rels.push([id, type, target]);
    return id;
  }
  addLink(url) {
    const id = this.#rid();
    this.rels.push([id, 'hyperlink', url, true]);
    return id;
  }
}

function parsePh(attr) {
  return { type: /type="(\w+)"/.exec(attr)?.[1], idx: /idx="(\d+)"/.exec(attr)?.[1] };
}

function slideXml(c, t, spec, profile, triggers) {
  const r = c.r;
  const area = { w: t.w, h: t.h };
  const tops = [];
  // placeholders of the layout
  for (const [attr] of spec.phs) {
    if (r.chance(0.08)) continue;
    const { type, idx } = parsePh(attr);
    const isTitle = type === 'title' || type === 'ctrTitle';
    tops.push(phXml(c, { type, idx, own: r.chance(0.25), area, texts: isTitle ? 1 : r.int(1, 5), levels: !isTitle && r.chance(0.5) }));
  }
  if (r.chance(0.15)) tops.push(phXml(c, { type: 'sldNum', idx: '4', own: false, area, texts: 1 }));
  // free shapes
  const n = r.int(profile.shapes[0], profile.shapes[1]);
  for (let i = 0; i < n; i++) {
    const kind = r.weighted([['sp', 52], ['tb', 14], ['pic', 8], ['grp', 8], ['cxn', 8], ['tbl', 6], ['sp3', 4]]);
    if (kind === 'sp') tops.push(spXml(c, area));
    else if (kind === 'sp3') tops.push(spXml(c, area, { text: false }));
    else if (kind === 'tb') tops.push(spXml(c, area, { textBox: true }));
    else if (kind === 'pic') tops.push(picXml(c, area));
    else if (kind === 'grp') tops.push(grpXml(c, area));
    else if (kind === 'cxn') tops.push(cxnXml(c, area));
    else tops.push(tableXml(c, area));
  }
  const ids = tops.map((x) => /<p:cNvPr id="(\d+)"/.exec(x)?.[1]).filter(Boolean);
  const animatable = tops.map((x, i) => (x.startsWith('<p:graphicFrame') ? null : ids[i])).filter(Boolean);
  const bg = r.chance(0.3) ? `<p:bg><p:bgPr>${fillXml(c)}<a:effectLst/></p:bgPr></p:bg>` : r.chance(0.05) ? '<p:bg><p:bgRef idx="1003"><a:schemeClr val="bg2"/></p:bgRef></p:bg>' : '';
  if (bg) c.feat('slide:bg');
  const tr = transitionXml(c);
  const timing = r.chance(0.5) ? timingXml(c, r.pickN(animatable, Math.min(animatable.length, r.int(1, 5))), triggers ? animatable : []) : '';
  const show = r.chance(0.04) ? ' show="0"' : '';
  const ovr = r.chance(0.05) ? '<p:clrMapOvr><a:overrideClrMapping bg1="dk1" tx1="lt1" bg2="dk2" tx2="lt2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:clrMapOvr>' : '<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>';
  if (show) c.feat('slide:hidden');
  return `${XML}<p:sld ${NS}${show}><p:cSld>${bg}<p:spTree>${GRP}${tops.join('')}</p:spTree></p:cSld>${ovr}${tr.xml}${timing}</p:sld>`;
}

/** Builds deck number `seed` of a profile; returns { bytes, features, slides }. */
export function buildDeck(seed, profile) {
  const r = new Rand(seed);
  const features = new Set();
  const media = [];
  const base = new PartCtx(r, features, media);
  const t = makeTemplate(base);
  const specs = layoutSpecs(t).slice(0, Math.max(t.layouts, 3)); // at least title/content/two-content
  const files = [];
  const add = (name, data, opt) => files.push([name, data, opt]);

  // master and layouts (rels: rId1 theme/master)
  const layoutRIds = specs.map((_, i) => `rId${i + 2}`);
  const master = masterXml(new PartCtx(r, features, media), t, layoutRIds);
  const layouts = specs.map((s, i) => layoutXml(new PartCtx(r, features, media), t, s, i));

  // slides
  const nSlides = r.int(profile.slides[0], profile.slides[1]);
  const slideParts = [];
  let notesMaster = false;
  for (let i = 0; i < nSlides; i++) {
    const c = new PartCtx(r, features, media);
    const li = r.below(specs.length);
    // a profile may bring its own slide builder (b3.mjs: text-heavy slides); the default is the B1 slide
    const xml = (profile.slide ?? slideXml)(c, t, specs[li], profile, true);
    const rels = [['rId1', 'slideLayout', `../slideLayouts/slideLayout${li + 1}.xml`], ...c.rels];
    let notes = null;
    if (r.chance(profile.notes)) {
      notes = `Notes for slide ${i + 1}\n${words(r, r.int(3, 12)).text}`;
      notesMaster = true;
      rels.push([`rId${rels.length + 1}`, 'notesSlide', `../notesSlides/notesSlide${i + 1}.xml`]);
      features.add('notes');
    }
    slideParts.push({ xml, rels, notes });
  }

  // presentation
  const presRels = [['rId1', 'slideMaster', 'slideMasters/slideMaster1.xml']];
  const slideRIds = slideParts.map((_, i) => { presRels.push([`rId${i + 2}`, 'slide', `slides/slide${i + 1}.xml`]); return `rId${i + 2}`; });
  const after = slideParts.length + 2;
  presRels.push([`rId${after}`, 'presProps', 'presProps.xml'], [`rId${after + 1}`, 'tableStyles', 'tableStyles.xml'], [`rId${after + 2}`, 'theme', 'theme/theme1.xml']);
  const notesMasterRId = notesMaster ? `rId${after + 3}` : null;
  if (notesMaster) presRels.push([notesMasterRId, 'notesMaster', 'notesMasters/notesMaster1.xml']);

  // zip order: content types first, then the package graph
  const parts = ['ppt/presentation.xml', 'ppt/presProps.xml', 'ppt/tableStyles.xml', 'ppt/theme/theme1.xml', 'ppt/slideMasters/slideMaster1.xml', ...layouts.map((_, i) => `ppt/slideLayouts/slideLayout${i + 1}.xml`), ...slideParts.map((_, i) => `ppt/slides/slide${i + 1}.xml`), ...(notesMaster ? ['ppt/notesMasters/notesMaster1.xml', 'ppt/theme/theme2.xml'] : []), ...slideParts.flatMap((s, i) => (s.notes ? [`ppt/notesSlides/notesSlide${i + 1}.xml`] : []))];
  add('[Content_Types].xml', contentTypes(parts));
  add('_rels/.rels', relsXml([['rId1', 'officeDocument', 'ppt/presentation.xml'], ['rId2', 'http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties', 'docProps/core.xml'], ['rId3', 'extended-properties', 'docProps/app.xml']]));
  add('docProps/app.xml', appXml(t, nSlides));
  add('docProps/core.xml', coreXml(`Generated deck ${seed}`));
  add('ppt/presentation.xml', presentationXml(t, slideRIds, { masterRId: 'rId1', notesMasterRId }));
  add('ppt/_rels/presentation.xml.rels', relsXml(presRels));
  add('ppt/presProps.xml', presPropsXml(t));
  add('ppt/tableStyles.xml', tableStylesXml());
  add('ppt/theme/theme1.xml', themeXml(t));
  add('ppt/slideMasters/slideMaster1.xml', master);
  add('ppt/slideMasters/_rels/slideMaster1.xml.rels', relsXml([['rId1', 'theme', '../theme/theme1.xml'], ...layoutRIds.map((rid, i) => [rid, 'slideLayout', `../slideLayouts/slideLayout${i + 1}.xml`])]));
  layouts.forEach((xml, i) => {
    add(`ppt/slideLayouts/slideLayout${i + 1}.xml`, xml);
    add(`ppt/slideLayouts/_rels/slideLayout${i + 1}.xml.rels`, relsXml([['rId1', 'slideMaster', '../slideMasters/slideMaster1.xml']]));
  });
  slideParts.forEach((s, i) => {
    add(`ppt/slides/slide${i + 1}.xml`, s.xml);
    add(`ppt/slides/_rels/slide${i + 1}.xml.rels`, relsXml(s.rels));
  });
  if (notesMaster) {
    add('ppt/notesMasters/notesMaster1.xml', notesMasterXml(t));
    add('ppt/notesMasters/_rels/notesMaster1.xml.rels', relsXml([['rId1', 'theme', '../theme/theme2.xml']]));
    add('ppt/theme/theme2.xml', themeXml(t, 'GenNotesTheme'));
  }
  slideParts.forEach((s, i) => {
    if (!s.notes) return;
    add(`ppt/notesSlides/notesSlide${i + 1}.xml`, notesSlideXml(s.notes));
    add(`ppt/notesSlides/_rels/notesSlide${i + 1}.xml.rels`, relsXml([['rId1', 'notesMaster', '../notesMasters/notesMaster1.xml'], ['rId2', 'slide', `../slides/slide${i + 1}.xml`]]));
  });
  for (const [name, png] of media) add(name, png, { store: true });
  return { bytes: zipBytes(files), features: [...features].sort(), slides: nSlides };
}
