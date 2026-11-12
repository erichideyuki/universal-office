import { zipSync, strToU8 } from 'fflate';
import { PptxPackage, readPresentation, readSlide, type Slide } from '../../src/index.js';

/**
 * Monta um .pptx mínimo nos testes (slides-core, viewer e editor): presentation
 * com N slides e, por padrão, um master + layout + tema de cores e fontes.
 * Com `template: false` saem só a presentation e os slides (sem tema).
 */

export const NS = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"';
export const REL = (id: string, type: string, target: string, extra = ''): string =>
  `<Relationship Id="${id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/${type}" Target="${target}"${extra}/>`;
const RELS = (body: string): Uint8Array => strToU8(`<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">${body}</Relationships>`);
/** `<p:cSld>` com o spTree contendo `s` (e um `bg` opcional antes dele). */
export const TREE = (s: string, bg = ''): string => `<p:cSld>${bg}<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>${s}</p:spTree></p:cSld>`;

const THEME_COLORS = '<a:clrScheme name="t"><a:dk1><a:srgbClr val="111111"/></a:dk1><a:lt1><a:srgbClr val="EEEEEE"/></a:lt1><a:dk2><a:srgbClr val="222222"/></a:dk2><a:lt2><a:srgbClr val="DDDDDD"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme>';
const FMT = '<a:fmtScheme name="f"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:gradFill><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="50000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"/></a:gs></a:gsLst><a:lin ang="5400000"/></a:gradFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="9525"/><a:ln w="12700"/><a:ln w="19050"/></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:gradFill><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"/></a:gs><a:gs pos="100000"><a:srgbClr val="000000"/></a:gs></a:gsLst><a:lin ang="5400000"/></a:gradFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme>';
const CLR_MAP = 'bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"';

export interface PptxSlideSpec {
  /** Formas do spTree do slide. */
  tree: string;
  /** Atributos/filhos extras do `<p:sld>` depois do cSld (ex.: clrMapOvr, timing). */
  root?: string;
  /** `show="0"`: slide oculto. */
  show?: boolean;
  /** Relações extras do slide (hiperlinks, imagens). */
  rels?: string;
}

export interface PptxDeckSpec {
  /** Slides em ordem (string = só o spTree). */
  slides: (PptxSlideSpec | string)[];
  /** Master + layout + tema (padrão true). */
  template?: boolean;
  layout?: string;
  master?: string;
  clrMap?: string;
  txStyles?: string;
  /** `<p:bg>` do master. */
  bg?: string;
  defaultTextStyle?: string;
  /** Partes extras do pacote (mídia). */
  media?: Record<string, Uint8Array>;
  /** Tamanho do slide em EMU (padrão 9144000 × 5143500). */
  size?: { cx: number; cy: number };
}

export interface BuiltPptx {
  bytes(): Uint8Array;
  pkg(): PptxPackage;
  /** Lê o slide `i` (0 = primeiro). */
  slide(i?: number): Slide;
}

export function buildPptx(d: PptxDeckSpec): BuiltPptx {
  const template = d.template ?? true;
  const slides = d.slides.map((s) => (typeof s === 'string' ? { tree: s } : s));
  const size = d.size ?? { cx: 9144000, cy: 5143500 };
  const ct = ['<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>',
    ...slides.map((_, i) => `<Override PartName="/ppt/slides/slide${i + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>`)];
  const sldIds = slides.map((_, i) => `<p:sldId id="${256 + i}" r:id="rIdS${i + 1}"/>`).join('');
  const presRels = slides.map((_, i) => REL(`rIdS${i + 1}`, 'slide', `slides/slide${i + 1}.xml`)).join('')
    + (template ? REL('rIdM', 'slideMaster', 'slideMasters/slideMaster1.xml') + REL('rIdT', 'theme', 'theme/theme1.xml') : '');
  const files: Record<string, Uint8Array> = {
    '[Content_Types].xml': strToU8(`<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>${ct.join('')}</Types>`),
    '_rels/.rels': RELS(REL('rId1', 'officeDocument', 'ppt/presentation.xml')),
    'ppt/presentation.xml': strToU8(`<?xml version="1.0"?><p:presentation ${NS}>${template ? '<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rIdM"/></p:sldMasterIdLst>' : ''}<p:sldIdLst>${sldIds}</p:sldIdLst><p:sldSz cx="${size.cx}" cy="${size.cy}"/>${d.defaultTextStyle ?? ''}</p:presentation>`),
    'ppt/_rels/presentation.xml.rels': RELS(presRels),
    ...(d.media ?? {}),
  };
  if (template) {
    files['ppt/theme/theme1.xml'] = strToU8(`<?xml version="1.0"?><a:theme ${NS} name="T"><a:themeElements>${THEME_COLORS}<a:fontScheme name="f"><a:majorFont><a:latin typeface="Major"/></a:majorFont><a:minorFont><a:latin typeface="Minor"/></a:minorFont></a:fontScheme>${FMT}</a:themeElements></a:theme>`);
    files['ppt/slideMasters/slideMaster1.xml'] = strToU8(`<?xml version="1.0"?><p:sldMaster ${NS}>${TREE(d.master ?? '', d.bg ?? '')}<p:clrMap ${d.clrMap ?? CLR_MAP}/>${d.txStyles ?? ''}</p:sldMaster>`);
    files['ppt/slideMasters/_rels/slideMaster1.xml.rels'] = RELS(REL('rId1', 'slideLayout', '../slideLayouts/slideLayout1.xml') + REL('rId2', 'theme', '../theme/theme1.xml'));
    files['ppt/slideLayouts/slideLayout1.xml'] = strToU8(`<?xml version="1.0"?><p:sldLayout ${NS}>${TREE(d.layout ?? '')}</p:sldLayout>`);
    files['ppt/slideLayouts/_rels/slideLayout1.xml.rels'] = RELS(REL('rId1', 'slideMaster', '../slideMasters/slideMaster1.xml'));
  }
  slides.forEach((s, i) => {
    const show = s.show === false ? ' show="0"' : '';
    files[`ppt/slides/slide${i + 1}.xml`] = strToU8(`<?xml version="1.0"?><p:sld ${NS}${show}>${TREE(s.tree)}${s.root ?? ''}</p:sld>`);
    const rels = (template ? REL('rId1', 'slideLayout', '../slideLayouts/slideLayout1.xml') : '') + (s.rels ?? '');
    if (rels) files[`ppt/slides/_rels/slide${i + 1}.xml.rels`] = RELS(rels);
  });
  const bytes = zipSync(files);
  const pkg = (): PptxPackage => PptxPackage.load(bytes);
  return {
    bytes: () => bytes,
    pkg,
    slide: (i = 0) => {
      const p = pkg();
      return readSlide(p, readPresentation(p).slides[i]!.target!);
    },
  };
}

/** Forma `<p:sp>` com texto (`body` = parágrafos `<a:p>`), placeholder e spPr opcionais. */
export const sp = (id: number, body: string, ph = '', spPr = '', cNvPr = ''): string =>
  `<p:sp><p:nvSpPr><p:cNvPr id="${id}" name="s${id}"${cNvPr}/><p:cNvSpPr/><p:nvPr>${ph}</p:nvPr></p:nvSpPr><p:spPr>${spPr}</p:spPr><p:txBody><a:bodyPr/><a:lstStyle/>${body}</p:txBody></p:sp>`;

/** `<a:xfrm>` em EMU. */
export const xfrm = (x: number, y: number, cx: number, cy: number, rot = 0): string =>
  `<a:xfrm${rot ? ` rot="${rot}"` : ''}><a:off x="${x}" y="${y}"/><a:ext cx="${cx}" cy="${cy}"/></a:xfrm>`;

/** Relação de hiperlink externo. */
export const hlinkRel = (id: string, url: string): string => REL(id, 'hyperlink', url, ' TargetMode="External"');
