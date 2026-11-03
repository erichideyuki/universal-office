// The generated attribute-quirk corpus of frontier R4b (inline objects: `w:drawing`, `w:pict`,
// `w:object`, `mc:AlternateContent`), on top of R1's `quirk-corpus.mjs` (`Gen`, `NUMS`, `FLAGS`,
// `WORDS`, `COLORS`): DrawingML inline/anchor pictures, text boxes, shapes and groups with the
// numbers, flags and ids real files and fuzzers put in attributes; SmartArt with its three parts
// (data model, colours, drawing); VML shapes, groups, images, WordArt, text boxes, lines, rules and
// OLE objects; both wrapped in `mc:AlternateContent`. `generate()` is deterministic.
//
//   node core/crates/uo-docx/js/quirk-objects.mjs     writes tests/parity/drawing-runs.quirks.txt
//
// A case is one line: the fragment text (under `w:root`), then for every part of the case
// `\u0001<rid>\u0002<part text>`; the whole line with `\`, LF, CR and TAB escaped. The capture
// (capture-drawing-runs.mjs) and the Rust replay (tests/parity_drawing_runs.rs) both read the file.
// A relationship id resolves to an image when its UTF-16 length is even and to nothing when odd;
// a part id resolves to the part of the case that carries it.
import { writeFileSync, readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Gen, NUMS, FLAGS, COLORS } from './quirk-corpus.mjs';

const here = dirname(fileURLToPath(import.meta.url));
export const CORPUS_FILE = join(here, '..', 'tests', 'parity', 'drawing-runs.quirks.txt');

const RIDS = ['rId1', 'rId22', 'rIdAB', 'abcd', 'rId333', 'rId4', 'x', 'rId10', 'a', '', 'rId', 'ab'];
const EMUS = ['0', '914400', '1905000', '952500', '635', '12700', '3175', '-635', '1e3', ' 635 ', '12pt', 'NaN', '', '99999999999999999999', '5400000', '60000', '100000', '2147483648'];
const PRSTS = ['rect', 'roundRect', 'ellipse', 'triangle', 'line', '', 'textPlain', 'custom', 'constructor'];
const VLENS = ['100pt', '2in', '5cm', '10mm', '1.5pc', '200px', '300', '7em', '12.5pt', ' 40pt ', '1e2', '-3pt', '', 'x', '99999999999999999999pt', '0'];
const VCOLORS = ['#ff0000', '#f00', 'red', 'window', '#12', '#xyz', '', 'silver', 'nope', '#ABCDEF'];
const TEXTS = ['', 'a', 'Olá mundo', '𝒜 b', 'x\ty', 'a&amp;b', 'a&lt;b', ' ', '­', 'é😀'];

export class ObjGen extends Gen {
  rid() { return this.pick(RIDS); }
  emu() { return this.pick(EMUS); }
  int() { return this.pick(NUMS); }
  flag() { return this.pick(FLAGS); }
  /** ` name="value"` or nothing (one in `miss`). */
  opt(name, pool, miss = 4) { return this.chance(miss) ? '' : ` ${name}="${this.pick(pool).replace(/"/g, '&quot;').replace(/</g, '&lt;')}"`; }

  clr() {
    const kind = this.pick(['srgbClr', 'schemeClr', 'sysClr', 'prstClr', 'scrgbClr', 'hslClr']);
    let a = '';
    if (kind === 'srgbClr') a = this.opt('val', ['4472c4', 'FF0000', 'zz', '', '12345', 'é']);
    else if (kind === 'schemeClr') a = this.opt('val', ['accent1', 'tx1', 'bg2', 'phClr', 'nope', 'tx2', 'bg1', 'constructor', '__proto__', 'toString', '']);
    else if (kind === 'sysClr') a = this.opt('val', ['window', 'windowText', '']) + this.opt('lastClr', ['FFFFFF', 'abc', ''], 2);
    else if (kind === 'prstClr') a = this.opt('val', ['white', 'black', 'red', 'gray', 'constructor', 'hasOwnProperty', '__proto__', '']);
    let mods = '';
    for (let k = this.rng.below(4); k > 0; k--) mods += `<a:${this.pick(['alpha', 'lumMod', 'lumOff', 'tint', 'shade', 'satMod', 'gray', 'bogus'])}${this.opt('val', ['75000', '25000', '0', '1e3', 'abc', '', ' 5000', '99999999999999999999', '-3'])}/>`;
    return mods ? `<a:${kind}${a}>${mods}</a:${kind}>` : `<a:${kind}${a}/>`;
  }

  fill() {
    return this.pick([
      () => '<a:noFill/>',
      () => `<a:solidFill>${this.clr()}</a:solidFill>`,
      () => `<a:solidFill>${this.clr()}</a:solidFill>`,
      () => `<a:gradFill><a:gsLst><a:gs pos="0">${this.clr()}</a:gs><a:gs pos="100000">${this.clr()}</a:gs></a:gsLst><a:lin${this.opt('ang', NUMS)}/></a:gradFill>`,
      () => `<a:pattFill${this.opt('prst', ['pct5', 'dkHorz', ''])}><a:fgClr>${this.clr()}</a:fgClr><a:bgClr>${this.clr()}</a:bgClr></a:pattFill>`,
      () => '',
    ])();
  }

  ln() {
    const kids = this.pick([
      () => '<a:noFill/>', () => `<a:solidFill>${this.clr()}</a:solidFill>`, () => `<a:gradFill><a:gsLst/></a:gradFill>`, () => `<a:pattFill/>`, () => '',
    ])() + (this.chance(2) ? `<a:prstDash${this.opt('val', ['dash', 'sysDot', 'solid', ''])}/>` : '')
      + this.pick([() => '<a:round/>', () => '<a:bevel/>', () => `<a:miter${this.opt('lim', NUMS)}/>`, () => '', () => ''])()
      + this.pick([() => `<a:headEnd type="arrow"/><a:tailEnd type="triangle" w="lg"/>`, () => '', () => '<a:extLst><a:ext uri="x"/></a:extLst>'])();
    return `<a:ln${this.opt('w', EMUS)}${this.opt('cap', ['rnd', 'sq', 'flat', ''])}${this.opt('cmpd', ['sng', 'dbl', 'tri', ''])}${this.opt('algn', ['ctr', 'in', ''])}>${kids}</a:ln>`;
  }

  effectLst() {
    let s = '';
    if (this.chance(2)) s += `<a:outerShdw${this.opt('blurRad', EMUS)}${this.opt('dist', EMUS)}${this.opt('dir', EMUS)}${this.opt('sx', EMUS, 3)}${this.opt('sy', EMUS, 3)}${this.opt('kx', EMUS, 3)}${this.opt('ky', EMUS, 3)}${this.opt('algn', ['b', 'tl', ''])}${this.opt('rotWithShape', FLAGS)}>${this.clr()}</a:outerShdw>`;
    if (this.chance(3)) s += `<a:innerShdw${this.opt('blurRad', EMUS)}${this.opt('dist', EMUS)}${this.opt('dir', EMUS)}>${this.clr()}</a:innerShdw>`;
    if (this.chance(4)) s += `<a:prstShdw${this.opt('prst', ['shdw1', 'shdw20', ''])}${this.opt('dist', EMUS)}${this.opt('dir', EMUS)}>${this.clr()}</a:prstShdw>`;
    if (this.chance(3)) s += `<a:glow${this.opt('rad', EMUS)}>${this.chance(4) ? '' : this.clr()}</a:glow>`;
    if (this.chance(3)) s += `<a:softEdge${this.opt('rad', EMUS)}/>`;
    if (this.chance(3)) s += `<a:reflection${['blurRad', 'stA', 'stPos', 'endA', 'endPos', 'dist', 'dir', 'fadeDir', 'sx', 'sy', 'kx', 'ky'].map((k) => this.opt(k, EMUS, 3)).join('')}${this.opt('algn', ['b', 'ctr', ''])}${this.opt('rotWithShape', FLAGS)}/>`;
    if (this.chance(5)) s += `<a:${this.pick(['blur', 'fillOverlay'])} x="1"/>`;
    return `<a:effectLst>${s}</a:effectLst>`;
  }

  scene3d() {
    return `<a:scene3d><a:camera${this.opt('prst', ['orthographicFront', ''])}${this.opt('fov', EMUS)}${this.opt('zoom', EMUS)}>${this.chance(2) ? `<a:rot${this.opt('lat', EMUS)}${this.opt('lon', EMUS)}${this.opt('rev', EMUS)}/>` : ''}</a:camera>`
      + `<a:lightRig${this.opt('rig', ['threePt', 'balanced', ''])}${this.opt('dir', ['t', 'tl', ''])}>${this.chance(2) ? `<a:rot${this.opt('lat', EMUS)}/>` : ''}</a:lightRig></a:scene3d>`
      + `<a:sp3d${this.opt('extrusionH', EMUS)}${this.opt('contourW', EMUS)}${this.opt('z', EMUS)}${this.opt('prstMaterial', ['matte', 'plastic', ''])}>${this.chance(2) ? `<a:bevelT${this.opt('w', EMUS)}${this.opt('h', EMUS)}${this.opt('prst', ['circle', ''])}/>` : ''}${this.chance(3) ? '<a:bevelB/>' : ''}${this.chance(3) ? `<a:extrusionClr>${this.clr()}</a:extrusionClr>` : ''}${this.chance(3) ? `<a:contourClr>${this.clr()}</a:contourClr>` : ''}</a:sp3d>`;
  }

  blip() {
    let kids = '';
    for (let k = this.rng.below(4); k > 0; k--) {
      kids += this.pick([
        () => `<a:alphaModFix${this.opt('amt', NUMS)}/>`, () => `<a:lum${this.opt('bright', NUMS)}${this.opt('contrast', NUMS)}/>`, () => '<a:grayscl/>',
        () => `<a:biLevel${this.opt('thresh', NUMS)}/>`, () => `<a:duotone>${this.chance(4) ? '' : this.clr()}${this.chance(3) ? '' : this.clr()}</a:duotone>`,
        () => `<a:clrChange${this.opt('useA', FLAGS)}><a:clrFrom>${this.clr()}</a:clrFrom>${this.chance(3) ? '' : `<a:clrTo>${this.clr()}</a:clrTo>`}</a:clrChange>`,
        () => `<a:extLst><a:ext uri="{BEBA8EAE-BF5A-486C-A8C5-ECC9F3942E4B}"><a14:imgProps><a14:imgLayer${this.opt('r:embed', RIDS)}><a14:imgEffect><a14:saturation${this.opt('sat', NUMS)}/></a14:imgEffect><a14:imgEffect><a14:colorTemperature${this.opt('colorTemp', NUMS)}/><a14:sharpenSoften${this.opt('amount', NUMS)}/><a14:brightnessContrast${this.opt('bright', NUMS)}${this.opt('contrast', NUMS)}/><a14:artisticBlur x="1"/></a14:imgEffect></a14:imgLayer></a14:imgProps></a:ext></a:extLst>`,
        () => `<a:extLst><a:ext uri="y"><asvg:svgBlip${this.opt('r:embed', RIDS)}/></a:ext></a:extLst>`,
      ])();
    }
    const svg = this.chance(3) ? `<asvg:svgBlip${this.opt('r:embed', RIDS)}/>` : '';
    return `<a:blip${this.opt('r:embed', RIDS, 6)}${this.chance(5) ? this.opt('r:link', RIDS) : ''}>${kids}${svg}</a:blip>`;
  }

  blipFill(tag = 'pic:blipFill') {
    const tile = this.chance(3) ? `<a:tile${this.opt('tx', NUMS)}${this.opt('ty', NUMS)}${this.opt('sx', NUMS)}${this.opt('sy', NUMS)}${this.opt('algn', ['tl', 'ctr', ''])}${this.opt('flip', ['none', 'xy', ''])}/>` : '<a:stretch><a:fillRect/></a:stretch>';
    const src = this.chance(2) ? `<a:srcRect${this.opt('l', NUMS)}${this.opt('t', NUMS)}${this.opt('r', NUMS)}${this.opt('b', NUMS)}/>` : '';
    return `<${tag}>${this.blip()}${src}${tile}</${tag}>`;
  }

  xfrm(group = false) {
    const ext = this.chance(5) ? '' : `<a:ext${this.opt('cx', EMUS)}${this.opt('cy', EMUS)}/>`;
    const off = this.chance(5) ? '' : `<a:off${this.opt('x', EMUS)}${this.opt('y', EMUS)}/>`;
    const ch = group ? `${this.chance(3) ? '' : `<a:chOff${this.opt('x', EMUS)}${this.opt('y', EMUS)}/>`}${this.chance(3) ? '' : `<a:chExt${this.opt('cx', EMUS)}${this.opt('cy', EMUS)}/>`}` : '';
    return `<a:xfrm${this.opt('rot', NUMS)}${this.opt('flipH', FLAGS)}${this.opt('flipV', FLAGS)}>${off}${ext}${ch}</a:xfrm>`;
  }

  geom() {
    if (this.chance(6)) return '<a:custGeom><a:pathLst><a:path w="100" h="100"><a:moveTo><a:pt x="0" y="0"/></a:moveTo><a:lnTo><a:pt x="100" y="100"/></a:lnTo><a:close/></a:path></a:pathLst></a:custGeom>';
    let av = '';
    for (let k = this.rng.below(3); k > 0; k--) av += `<a:gd${this.opt('name', ['adj', 'adj1', '1', '__proto__', 'constructor', ''], 8)}${this.opt('fmla', ['val 5000', 'val -12', 'val  7', 'VAL 1', 'val 1x', '', 'val 12345678901234567890'], 6)}/>`;
    return `<a:prstGeom${this.opt('prst', PRSTS, 5)}><a:avLst>${av}</a:avLst></a:prstGeom>`;
  }

  spPr(tag, { blipfill = false } = {}) {
    const bf = blipfill ? this.blipFill('a:blipFill') : '';
    return `<${tag}>${this.chance(5) ? '' : this.xfrm()}${this.geom()}${bf || (this.chance(6) ? '' : this.fill())}${this.chance(3) ? this.ln() : ''}${this.chance(3) ? this.effectLst() : ''}${this.chance(5) ? this.scene3d() : ''}</${tag}>`;
  }

  style() {
    const ref = (n) => `<a:${n}${this.opt('idx', ['0', '1', '2', '3', '1001', 'x', 'major', 'minor', ''])}>${this.chance(3) ? '' : this.clr()}</a:${n}>`;
    return `<wps:style>${ref('lnRef')}${ref('fillRef')}${ref('effectRef')}${ref('fontRef')}</wps:style>`;
  }

  paras() {
    let s = '';
    for (let k = this.rng.below(4); k > 0; k--) s += this.chance(6) ? '<w:tbl/>' : `<w:p><w:r><w:t>${this.pick(TEXTS)}</w:t></w:r></w:p>`;
    return s;
  }

  bodyPr() {
    return `<wps:bodyPr${this.opt('wrap', ['none', 'square', ''])}${this.opt('lIns', EMUS, 3)}${this.opt('tIns', EMUS, 3)}${this.opt('rIns', EMUS, 3)}${this.opt('bIns', EMUS, 3)}${this.opt('anchor', ['t', 'ctr', 'b', 'x', ''])}${this.opt('vert', ['horz', 'vert', 'vert270', 'eaVert', ''])}${this.opt('upright', FLAGS)}${this.opt('fromWordArt', FLAGS)}>${this.chance(3) ? `<a:prstTxWarp${this.opt('prst', PRSTS)}/>` : ''}${this.chance(3) ? '<a:spAutoFit/>' : this.chance(4) ? '<a:p><a:spAutoFit/></a:p>' : ''}</wps:bodyPr>`;
  }

  wsp() {
    const tb = this.pick([
      () => `<wps:txbx${this.opt('id', NUMS)}><w:txbxContent>${this.paras()}</w:txbxContent></wps:txbx>`,
      () => `<wps:txbx><w:txbxContent>${this.paras()}</w:txbxContent></wps:txbx>`,
      () => '<wps:txbx/>', () => `<wps:linkedTxbx${this.opt('id', NUMS)}${this.opt('seq', NUMS)}/>`, () => '', () => '',
    ])();
    return `<wps:wsp>${this.spPr('wps:spPr', { blipfill: this.chance(5) })}${this.chance(2) ? this.style() : ''}${tb}${this.chance(5) ? '' : this.bodyPr()}</wps:wsp>`;
  }

  pic() {
    return `<pic:pic><pic:nvPicPr/>${this.chance(8) ? '' : this.blipFill()}${this.chance(5) ? '' : this.spPr('pic:spPr')}</pic:pic>`;
  }

  grp(depth = 0) {
    let kids = '';
    for (let k = this.rng.below(4); k > 0; k--) {
      kids += this.pick([() => this.wsp(), () => this.pic(), () => (depth < 3 ? this.grp(depth + 1) : this.wsp()), () => this.wsp(), () => '<wps:other/>'])();
    }
    const tag = depth === 0 ? this.pick(['wpg:wgp', 'wpg:wgp', 'wpc:wpc']) : 'wpg:grpSp';
    return `<${tag}><wpg:grpSpPr>${this.chance(5) ? '' : this.xfrm(true)}</wpg:grpSpPr>${kids}</${tag}>`;
  }

  diagram(parts) {
    const n = this.rng.below(1000000);
    const rdm = `rIdD${n}`;
    const rcs = `rIdC${n}`;
    const rdr = `rIdR${n}`;
    const pts = Array.from({ length: 1 + this.rng.below(3) }, (_, i) => `<dgm:pt modelId="m${i}"><dgm:prSet${this.opt('presStyleLbl', ['node0', 'node1', 'x', ''])}${this.opt('presStyleIdx', ['0', '1', '2', '5', '-1', '1e3', 'x', '', '99999999999999999999'], 3)}/></dgm:pt>`).join('');
    const lbl = (name) => `<dgm:styleLbl name="${name}"><dgm:txFillClrLst${this.opt('meth', ['cycle', 'repeat', ''])}>${Array.from({ length: this.rng.below(4) }, () => (this.chance(7) ? '<a:foo/>' : this.clr())).join('')}</dgm:txFillClrLst></dgm:styleLbl>`;
    const data = this.chance(8) ? null : `<dgm:dataModel><dgm:ptLst>${pts}</dgm:ptLst><dgm:extLst><a:ext><dsp:dataModelExt${this.chance(8) ? '' : ` relId="${rdr}"`}/></a:ext></dgm:extLst></dgm:dataModel>`;
    const colors = this.chance(6) ? null : `<dgm:colorsDef>${lbl('node0')}${lbl('node1')}</dgm:colorsDef>`;
    const run = () => `<a:r><a:rPr${this.opt('sz', ['1800', '1200', 'x', '', '0', '-5', '1e3'])}${this.opt('b', FLAGS)}${this.opt('i', FLAGS)}${this.opt('u', ['sng', 'dbl', 'none', ''])}>${this.chance(3) ? `<a:solidFill>${this.clr()}</a:solidFill>` : ''}${this.chance(3) ? `<a:latin typeface="${this.pick(['Calibri', '+mj-lt', '+mn-lt', ''])}"/>` : ''}${this.chance(4) ? `<a:ea typeface="${this.pick(['MS Gothic', '+mn-ea'])}"/>` : ''}${this.chance(4) ? `<a:cs typeface="${this.pick(['Arial', '+mj-cs'])}"/>` : ''}</a:rPr><a:t>${this.pick(TEXTS)}</a:t></a:r>`;
    const para = () => `<a:p><a:pPr${this.opt('algn', ['ctr', 'r', 'just', 'dist', 'l', ''])}${this.opt('marL', EMUS)}${this.opt('indent', EMUS)}>${this.chance(3) ? `<a:lnSpc>${this.chance(2) ? `<a:spcPct${this.opt('val', NUMS)}/>` : `<a:spcPts${this.opt('val', NUMS)}/>`}</a:lnSpc>` : ''}${this.chance(3) ? `<a:spcBef>${this.pick([() => `<a:spcPts${this.opt('val', NUMS)}/>`, () => `<a:spcPct${this.opt('val', NUMS)}/>`])()}</a:spcBef>` : ''}${this.chance(3) ? `<a:spcAft><a:spcPts${this.opt('val', NUMS)}/></a:spcAft>` : ''}</a:pPr>`
      + Array.from({ length: this.rng.below(3) }, () => this.pick([run, run, () => '<a:br/>', () => `<a:fld><a:t>${this.pick(TEXTS)}</a:t></a:fld>`, () => '<a:other/>']))
        .map((f) => f()).join('') + `${this.chance(3) ? `<a:endParaRPr${this.opt('sz', ['2400', 'x'])}/>` : ''}</a:p>`;
    const sp = () => `<dsp:sp${this.opt('modelId', ['m0', 'm1', 'm2', 'x', ''])}>${this.chance(7) ? '' : `<dsp:spPr>${this.chance(6) ? '' : this.xfrm()}${this.geom()}${this.fill()}${this.chance(3) ? this.ln() : ''}</dsp:spPr>`}${this.chance(2) ? `<dsp:style><a:fillRef idx="1">${this.clr()}</a:fillRef><a:fontRef${this.opt('idx', ['minor', 'major', ''])}>${this.chance(3) ? '' : this.clr()}</a:fontRef></dsp:style>` : ''}`
      + `${this.chance(4) ? '' : `<dsp:txBody><a:bodyPr${this.opt('spcFirstLastPara', FLAGS)}${this.opt('lIns', EMUS)}/>${Array.from({ length: this.rng.below(3) }, para).join('')}</dsp:txBody>`}${this.chance(3) ? `<dsp:txXfrm${this.opt('rot', NUMS)}><a:off${this.opt('x', EMUS)}${this.opt('y', EMUS)}/><a:ext${this.opt('cx', EMUS)}${this.opt('cy', EMUS)}/></dsp:txXfrm>` : ''}</dsp:sp>`;
    const tree = `<dsp:drawing><dsp:spTree>${Array.from({ length: this.rng.below(4) }, () => (this.chance(5) ? `<dsp:grpSp>${sp()}${sp()}</dsp:grpSp>` : sp())).join('')}</dsp:spTree></dsp:drawing>`;
    if (data) parts.push([rdm, data]);
    if (colors) parts.push([rcs, colors]);
    if (!this.chance(10)) parts.push([rdr, tree]);
    return `<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/diagram"><dgm:relIds${this.chance(10) ? '' : ` r:dm="${rdm}"`}${this.chance(5) ? '' : ` r:cs="${rcs}"`}/></a:graphicData></a:graphic>`;
  }

  drawing(parts) {
    const inline = this.chance(2);
    const tag = inline ? 'wp:inline' : 'wp:anchor';
    const obj = this.pick([() => this.wsp(), () => this.wsp(), () => this.wsp(), () => this.pic(), () => this.pic(), () => this.pic(), () => this.grp(), () => this.grp(), () => this.grp(), () => '', () => '<foo:bar/>']);
    const uri = this.pick(['http://schemas.microsoft.com/office/word/2010/wordprocessingShape', 'http://schemas.openxmlformats.org/drawingml/2006/picture', 'http://schemas.microsoft.com/office/word/2010/wordprocessingGroup', 'x', '']);
    const dgm = this.chance(6);
    const gd = dgm ? this.diagram(parts) : `<a:graphic><a:graphicData${this.opt('uri', [uri], 3)}>${obj()}</a:graphicData></a:graphic>`;
    const pos = (t, k) => (this.chance(4) ? '' : `<${t}${this.opt('relativeFrom', ['page', 'margin', 'column', 'paragraph', 'x', ''])}>${this.pick([() => `<wp:align>${this.pick(['left', 'center', ''])}</wp:align>`, () => `<wp:posOffset>${this.pick(NUMS)}</wp:posOffset>`, () => `<${k}>${this.pick(NUMS)}</${k}>`, () => ''])()}</${t}>`);
    const wrap = this.pick([() => '<wp:wrapNone/>', () => '<wp:wrapSquare wrapText="bothSides"/>', () => `<wp:wrapTight>${this.chance(2) ? '<wp:wrapPolygon edited="0"><wp:start x="0" y="0"/><wp:lineTo x="0" y="21600"/><wp:lineTo x="21600" y="21600"/><wp:lineTo x="21600" y="0"/><wp:lineTo x="0" y="0"/></wp:wrapPolygon>' : ''}</wp:wrapTight>`, () => '<wp:wrapTopAndBottom/>', () => ''])();
    const attrs = inline
      ? `${this.opt('distT', EMUS)}${this.opt('distL', EMUS)}`
      : `${this.opt('behindDoc', FLAGS)}${this.opt('distT', EMUS)}${this.opt('distB', EMUS)}${this.opt('distL', EMUS)}${this.opt('distR', EMUS)}${this.opt('relativeHeight', EMUS)}${this.opt('layoutInCell', FLAGS)}${this.opt('allowOverlap', FLAGS)}${this.opt('locked', FLAGS)}`;
    return `<w:drawing><${tag}${attrs}>${inline ? '' : `${pos('wp:positionH', 'wp14:pctPosHOffset')}${pos('wp:positionV', 'wp14:pctPosVOffset')}`}${this.chance(8) ? '' : `<wp:extent${this.opt('cx', EMUS)}${this.opt('cy', EMUS)}/>`}`
      + `${this.chance(3) ? `<wp:effectExtent${this.opt('l', EMUS)}${this.opt('t', EMUS)}${this.opt('r', EMUS)}${this.opt('b', EMUS)}/>` : ''}${inline ? '' : wrap}`
      + `<wp:docPr${this.opt('id', ['1', '2', ''])}${this.opt('name', ['Imagem 1', 'a&amp;b', ''])}${this.opt('descr', ['alt', 'a&quot;b', ''])}${this.opt('title', ['t', ''])}${this.opt('hidden', FLAGS)}>${this.chance(5) ? '<a:extLst><a:ext><adec:decorative val="1"/></a:ext></a:extLst>' : ''}</wp:docPr>`
      + `${this.chance(4) ? `<wp:cNvGraphicFramePr><a:graphicFrameLocks${this.opt('noChangeAspect', FLAGS)}/></wp:cNvGraphicFramePr>` : ''}${gd}${this.chance(4) ? `<wp14:sizeRelH${this.opt('relativeFrom', ['page', 'margin', ''])}><wp14:pctWidth>${this.pick(NUMS)}</wp14:pctWidth></wp14:sizeRelH>` : ''}</${tag}></w:drawing>`;
  }

  // ── VML ──
  vstyle() {
    const decls = [];
    const put = (k, v) => { if (!this.chance(4)) decls.push(`${k}:${v}`); };
    put('position', this.pick(['absolute', 'absolute', 'relative', '']));
    put('margin-left', this.pick(VLENS)); put('margin-top', this.pick(VLENS)); put('left', this.pick(VLENS)); put('top', this.pick(VLENS));
    put('width', this.pick(VLENS)); put('height', this.pick(VLENS)); put('z-index', this.pick(NUMS));
    put('mso-position-horizontal', this.pick(['center', 'absolute', 'left', 'constructor', '']));
    put('mso-position-vertical', this.pick(['center', 'absolute', 'bottom', '']));
    put('mso-position-horizontal-relative', this.pick(['page', 'margin', 'text', 'char', 'left-margin-area', 'toString', 'nope', '']));
    put('mso-position-vertical-relative', this.pick(['page', 'margin', 'text', 'line', 'top-margin-area', 'hasOwnProperty', 'nope', '']));
    put('mso-wrap-distance-left', this.pick(VLENS)); put('mso-wrap-distance-top', this.pick(VLENS));
    put('rotation', this.pick(NUMS)); put('flip', this.pick(['x', 'y', 'xy', '']));
    put('visibility', this.pick(['hidden', 'visible'])); put('display', this.pick(['none', 'block']));
    put('v-text-anchor', this.pick(['middle', 'bottom', 'top', 'bottom-baseline', 'middle-center', '']));
    put('mso-wrap-style', this.pick(['none', 'square']));
    return decls.join(this.pick([';', ' ; ']));
  }

  vshape(inGroup = false) {
    const tag = this.pick(['v:shape', 'v:shape', 'v:rect', 'v:roundrect', 'v:oval', 'v:line']);
    const attrs = this.opt('id', ['_x0000_s1026', 'Caixa 1', 'PowerPlusWaterMarkObject1', '', 'a&amp;b'])
      + this.opt('o:spid', ['_x0000_s1026', 'x', '', '_x0000_s99999999999999999999'])
      + this.opt('type', ['#_x0000_t75', '#_x0000_t202', '#_x0000_t32', '', 'x', '', ''], 2)
      + this.opt('style', [this.vstyle()], 8)
      + this.opt('filled', ['f', 'false', 't', '', 'true'], 4) + this.opt('stroked', ['f', 'false', 't', ''], 4)
      + this.opt('fillcolor', VCOLORS, 5) + this.opt('strokecolor', VCOLORS, 3) + this.opt('strokeweight', ['1pt', '2', '.5pt', '3t', '', '1e3'], 3)
      + this.opt('arcsize', ['.25', '5461f', 'x', '', '0'], 3) + this.opt('alt', ['texto', 'a&quot;b&lt;c', ''], 3)
      + (tag === 'v:line' ? this.opt('from', ['0,0', '10pt,20pt', ',', '1in', 'x,y', ' 1pt , 2pt ', ''], 2) + this.opt('to', ['100pt,50pt', '1in,1in', '5', ''], 2) : '')
      + this.opt('o:hr', ['t', 'f', ''], 6) + this.opt('o:hrpct', ['990', '1000', '-5', 'x', '0', ''], 3) + this.opt('o:hrnoshade', ['t', 'f'], 3) + this.opt('o:hralign', ['center', 'left', ''], 3);
    const kids = (this.chance(3) ? `<v:fill${this.opt('color', VCOLORS, 3)}${this.opt('opacity', ['.5', '32768f', 'x', '', '1', 'f'], 3)}${this.opt('type', ['gradient', 'solid', 'frame'])}${this.opt('angle', NUMS)}/>` : '')
      + (this.chance(3) ? `<v:stroke${this.opt('color', VCOLORS, 3)}${this.opt('weight', ['1pt', '2'], 3)}${this.opt('dashstyle', ['dash', 'solid', ''])}${this.opt('linestyle', ['single', 'thinThin', 'constructor', ''])}/>` : '')
      + this.pick([
        () => `<v:imagedata${this.opt('r:id', RIDS)}${this.opt('r:pict', RIDS, 6)}${this.opt('cropleft', ['10%', '5000f', '.1', 'x', '', '-.2'], 3)}${this.opt('croptop', ['.1', '3277f', ''], 3)}${this.opt('cropright', ['.1', ''], 3)}${this.opt('cropbottom', ['.1', ''], 3)}${this.opt('gain', ['19661f', '1', '2', 'x', '65536f', ''], 3)}${this.opt('blacklevel', ['22938f', '0', '.1', 'x', ''], 3)}${this.opt('grayscale', ['t', 'f', 'true'], 4)}${this.opt('bilevel', ['t', 'f', 'true'], 4)}/>`,
        () => `<v:textpath${this.opt('string', ['RASCUNHO', ' ', '', 'A &amp; B', 'é😀'])}${this.opt('style', ['font-family:&quot;Calibri&quot;;font-size:1pt;font-weight:bold', 'font-family:"Arial";font-style:italic', "font-family:'Times';font-weight:700", 'font-weight:600;font-family:;', 'font-weight:500'])}/>`,
        () => `<v:textbox${this.opt('style', ['layout-flow:vertical;mso-layout-flow-alt:bottom-to-top', 'layout-flow:vertical;mso-layout-flow-alt:top-to-bottom', 'layout-flow:vertical', 'layout-flow:vertical-ideographic', 'mso-fit-shape-to-text:t', 'mso-next-textbox:#_x0000_s1027', 'mso-next-textbox:x', ''])}${this.opt('inset', ['0,0,0,0', '7.2pt,3.6pt', ',,,', ' 1pt , 2pt ', 'x,y', '1in,,2cm,1e3', '0.1in,0.05in,0.1in,0.05in'])}><w:txbxContent>${this.paras()}</w:txbxContent></v:textbox>`,
        () => '<v:textbox/>', () => '', () => '',
      ])()
      + (this.chance(3) ? `<w10:wrap${this.opt('type', ['square', 'tight', 'through', 'topAndBottom', 'none', ''])}/>` : '');
    void inGroup;
    return `<${tag}${attrs}>${kids}</${tag}>`;
  }

  vgroup() {
    let kids = '';
    for (let k = this.rng.below(4); k > 0; k--) kids += this.pick([() => this.vshape(true), () => this.vshape(true), () => '<v:shapetype id="x"/>', () => '<o:lock/>', () => `<v:group${this.opt('style', [this.vstyle()])}>${this.vshape(true)}</v:group>`])();
    return `<v:group${this.opt('style', [this.vstyle()], 6)}${this.opt('coordsize', ['1000,1000', '21600,21600', 'x,y', '0,0', ',', '100', '1e3,2e3', ''])}${this.opt('coordorigin', ['0,0', '100,200', 'x', '-5,-5', ''])}>${kids}${this.chance(3) ? `<w10:wrap${this.opt('type', ['square', 'none'])}/>` : ''}</v:group>`;
  }

  pict() {
    const inner = this.pick([() => this.vshape(), () => this.vshape(), () => this.vshape() + this.vshape(), () => this.vgroup(), () => this.vgroup(), () => '<v:shapetype id="_x0000_t75"/>' + this.vshape(), () => '<o:OLEObject/>']);
    return `<w:pict>${inner()}</w:pict>`;
  }

  object() {
    const ole = this.chance(2) ? `<o:OLEObject${this.opt('Type', ['Embed', 'Link', ''])}${this.opt('ProgID', ['Excel.Sheet.12', 'Word.Document.12', ''])}${this.opt('r:id', RIDS)}/>` : '';
    return `<w:object>${this.vshape()}${ole}</w:object>`;
  }

  /** A `w:pict` with one text box shape: the generated `wps` text box (`rawDrawingXml`) and its edits. */
  vtb() {
    const INJECT = ['&lt;/wp:align&gt;&lt;x/&gt;', 'a&amp;b', 'c&lt;d', '&lt;/wp:positionH&gt;&lt;wp:positionH relativeFrom=&quot;page&quot;&gt;', 'left', 'center', '&quot;&gt;&lt;a:x/&gt;&lt;a:y a=&quot;'];
    const decls = ['position:absolute', `width:${this.pick(VLENS)}`, `height:${this.pick(VLENS)}`];
    const put = (k, vals, miss = 3) => { if (!this.chance(miss)) decls.push(`${k}:${this.pick(vals)}`); };
    put('margin-left', VLENS); put('margin-top', VLENS); put('z-index', NUMS);
    put('mso-position-horizontal', this.chance(4) ? INJECT : ['center', 'absolute', 'left', 'right', 'inside', ''], 2);
    put('mso-position-vertical', this.chance(4) ? INJECT : ['center', 'absolute', 'top', 'bottom'], 2);
    put('mso-position-horizontal-relative', ['page', 'margin', 'text', 'char', 'left-margin-area', 'constructor', '__proto__', 'toString', 'valueOf', 'nope', ''], 2);
    put('mso-position-vertical-relative', ['page', 'margin', 'text', 'line', 'top-margin-area', 'isPrototypeOf', 'nope'], 2);
    put('mso-wrap-distance-left', VLENS); put('rotation', ['45', '-30', '0', '725', 'x', '0.4']); put('flip', ['x', 'y', 'xy', '']);
    const tag = this.pick(['v:shape', 'v:rect', 'v:roundrect', 'v:oval']);
    const attrs = this.opt('id', ['_x0000_s1026', 'Caixa 1', 'a&amp;b&quot;c', '']) + this.opt('o:spid', ['_x0000_s1026', '_x0000_s2', 'x', ''])
      + this.opt('style', [decls.join(';')], 20) + this.opt('filled', ['f', 't', ''], 3) + this.opt('stroked', ['f', 't', ''], 3)
      + this.opt('fillcolor', VCOLORS, 3) + this.opt('strokecolor', VCOLORS, 3) + this.opt('strokeweight', ['1pt', '2', '.5pt', '3t', '12px'], 2)
      + this.opt('arcsize', ['.25', '5461f', 'x', '', '0'], 2) + this.opt('alt', ['texto', 'a&quot;b&lt;c', '&lt;/wp:docPr&gt;'], 2) + this.opt('o:title', ['t', 'a&amp;b'], 2);
    const kids = `<v:fill${this.opt('color', VCOLORS, 3)}${this.opt('color2', VCOLORS, 3)}${this.opt('opacity', ['.5', '32768f', 'x', '1', '-.5'], 3)}${this.opt('type', ['gradient', 'gradientRadial', 'solid'])}${this.opt('angle', NUMS)}/>`
      + `<v:stroke${this.opt('on', ['f', 't'], 4)}${this.opt('color', VCOLORS, 3)}${this.opt('weight', ['1pt', '2'], 3)}${this.opt('dashstyle', ['dash', 'solid', 'a&quot;b', 'x&lt;y', 'a&amp;b', '', 'dot', '&quot;/&gt;&lt;a:x/&gt;&lt;a:y a=&quot;'])}${this.opt('linestyle', ['single', 'thinThin', 'thickThin', 'constructor', '__proto__', 'nope', ''])}/>`
      + `<v:textbox${this.opt('style', ['layout-flow:vertical;mso-layout-flow-alt:bottom-to-top', 'layout-flow:vertical', 'mso-fit-shape-to-text:t', ''])}${this.opt('inset', ['0,0,0,0', '7.2pt,3.6pt', ' 1pt , 2pt ', '1in,,2cm,1e3'])}><w:txbxContent>${this.paras()}</w:txbxContent></v:textbox>`
      + (this.chance(2) ? `<w10:wrap${this.opt('type', ['square', 'tight', 'through', 'topAndBottom', 'none', ''])}/>` : '');
    return `<w:pict>${this.chance(3) ? '<v:shapetype id="_x0000_t202"/>' : ''}<${tag}${attrs}>${kids}</${tag}></w:pict>`;
  }

  alternate() {
    return `<mc:AlternateContent>${this.chance(8) ? '' : `<mc:Choice Requires="wps">${this.chance(5) ? '' : this.drawing([])}</mc:Choice>`}${this.chance(4) ? '' : `<mc:Fallback>${this.pick([() => this.pict(), () => this.object(), () => ''])()}</mc:Fallback>`}</mc:AlternateContent>`;
  }
}

const DM = (pts, ext = '<dsp:dataModelExt relId="rIdDr"/>') => `<dgm:dataModel><dgm:ptLst>${pts}</dgm:ptLst><dgm:extLst><a:ext>${ext}</a:ext></dgm:extLst></dgm:dataModel>`;
const DIAGRAM = (rel = 'r:dm="rIdDm" r:cs="rIdCs"') => `<w:drawing><wp:inline><wp:extent cx="3810000" cy="2540000"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/diagram"><dgm:relIds ${rel}/></a:graphicData></a:graphic></wp:inline></w:drawing>`;
const SP = (id, extra = '') => `<dsp:sp modelId="${id}"><dsp:spPr><a:xfrm><a:off x="635000" y="0"/><a:ext cx="1905000" cy="952500"/></a:xfrm><a:prstGeom prst="roundRect"/><a:solidFill><a:srgbClr val="4472C4"/></a:solidFill><a:ln w="12700"><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></a:ln></dsp:spPr><dsp:style><a:fillRef idx="1"><a:schemeClr val="accent1"/></a:fillRef><a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef></dsp:style><dsp:txBody><a:bodyPr spcFirstLastPara="0" lIns="76200"/><a:p><a:pPr algn="ctr"><a:lnSpc><a:spcPct val="90000"/></a:lnSpc><a:spcBef><a:spcPct val="0"/></a:spcBef><a:spcAft><a:spcPts val="600"/></a:spcAft></a:pPr><a:r><a:rPr sz="2400" b="1"><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill><a:latin typeface="+mj-lt"/></a:rPr><a:t>Etapa ${id}</a:t></a:r><a:br/><a:r><a:rPr sz="1200" u="dbl"/><a:t>detalhe</a:t></a:r></a:p><a:p><a:endParaRPr sz="1000"/></a:p></dsp:txBody><dsp:txXfrm rot="5400000"><a:off x="650000" y="10000"/><a:ext cx="1800000" cy="900000"/></dsp:txXfrm>${extra}</dsp:sp>`;
const COLORS_DEF = (meth) => `<dgm:colorsDef><dgm:styleLbl name="node1"><dgm:txFillClrLst meth="${meth}"><a:srgbClr val="FFFFFF"/><a:schemeClr val="accent2"><a:tint val="60000"/><a:shade val="50000"/></a:schemeClr><a:sysClr val="windowText" lastClr="111111"/></dgm:txFillClrLst></dgm:styleLbl></dgm:colorsDef>`;
const PT = (id, idx) => `<dgm:pt modelId="${id}"><dgm:prSet presStyleLbl="node1" presStyleIdx="${idx}"/></dgm:pt>`;
const DRAW = (inner) => `<dsp:drawing><dsp:spTree>${inner}</dsp:spTree></dsp:drawing>`;
const OLE = (type) => `<w:object><v:shape id="_x0000_i1025" type="#_x0000_t75" style="width:100pt;height:50pt" o:ole=""><v:imagedata r:id="rIdIMG" o:title="" cropleft="5000f" gain="19661f"/></v:shape><o:OLEObject Type="${type}" ProgID="Excel.Sheet.12" ShapeID="_x0000_i1025" DrawAspect="Content" ObjectID="_1" r:id="rIdOLE"/></w:object>`;

/** Hand-written cases: well-formed SmartArt (every colour kind, cycle and repeat), its failures, OLE objects. */
function fixed() {
  const cases = [];
  const add = (name, frag, parts = []) => cases.push([`quirk/fixed-${name}`, `<w:root>${frag}</w:root>`, parts]);
  const good = (meth, idxs) => [['rIdDm', DM(idxs.map((i, k) => PT(`m${k}`, i)).join(''))], ['rIdCs', COLORS_DEF(meth)], ['rIdDr', DRAW(SP('m0') + `<dsp:grpSp>${SP('m1')}${SP('m2')}</dsp:grpSp>` + SP('m3'))]];
  add('smartart-cycle', DIAGRAM(), good('cycle', ['0', '1', '2', '3']));
  add('smartart-repeat', DIAGRAM(), good('repeat', ['0', '1', '5', '99']));
  add('smartart-negative-cycle', DIAGRAM(), good('cycle', ['0', '-1']));
  add('smartart-negative-repeat', DIAGRAM(), good('repeat', ['-3']));
  add('smartart-huge', DIAGRAM(), good('cycle', ['9'.repeat(400)]));
  add('smartart-nodrawing', DIAGRAM(), [['rIdDm', DM(PT('m0', '0'), '<dsp:dataModelExt relId="rIdZZ"/>')]]);
  add('smartart-anchor', DIAGRAM().replace('<wp:inline>', '<wp:anchor behindDoc="1"><wp:positionH relativeFrom="page"><wp:posOffset>100</wp:posOffset></wp:positionH>').replace('</wp:inline>', '</wp:anchor>').replace('<wp:extent cx="3810000" cy="2540000"/>', '<wp:extent cx="3810000" cy="2540000"/><wp:wrapSquare/>'), good('cycle', ['0']));
  add('smartart-effectextent', DIAGRAM().replace('<wp:extent cx="3810000" cy="2540000"/>', '<wp:extent cx="3810000" cy="2540000"/><wp:effectExtent l="63500" t="127000" r="0" b="635"/>'), good('repeat', ['1']));
  add('smartart-noparts', DIAGRAM(), []);
  add('ole-embed', OLE('Embed'), [['rIdOLE', '<x/>']]);
  add('ole-link', OLE('Link'));
  add('ole-missing-image', OLE('Embed').replace('rIdIMG', 'rIdIM'));
  add('ole-no-part', OLE('Embed'));
  add('object-textbox', '<w:object><v:rect style="width:10pt;height:5pt"><v:textbox><w:txbxContent><w:p/></w:txbxContent></v:textbox></v:rect><o:OLEObject Type="Embed" r:id="rIdOLE"/></w:object>', [['rIdOLE', '<x/>']]);
  return cases;
}

/** The cases: `[key, fragment, parts]` with `parts` = `[[rid, text], …]`. */
export function generate(n = 2200, seed = 0x0b9ec7) {
  const g = new ObjGen(seed);
  const out = fixed();
  for (let i = 0; i < n; i++) {
    const parts = [];
    const frag = g.pick([
      () => g.drawing(parts), () => g.drawing(parts), () => g.drawing(parts), () => g.drawing(parts), () => g.drawing(parts),
      () => g.pict(), () => g.pict(), () => g.vtb(), () => g.vtb(), () => g.vtb(), () => g.object(), () => g.alternate(),
    ])();
    out.push([`quirk/obj-${String(i).padStart(4, '0')}`, `<w:root>${frag}</w:root>`, parts]);
  }
  return out;
}

const esc = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
const unesc = (x) => x.replace(/\\(.)/gs, (_, c) => (c === 'n' ? '\n' : c === 'r' ? '\r' : c === 't' ? '\t' : c));

/** The committed corpus: `[key, fragment, parts]`. */
export function readCorpus(file = CORPUS_FILE) {
  return readFileSync(file, 'utf8').split('\n').filter((l) => l && !l.startsWith('#')).map((l) => {
    const [key, text] = [l.slice(0, l.indexOf('\t')), unesc(l.slice(l.indexOf('\t') + 1))];
    const [frag, ...rest] = text.split('\u0001');
    return [key, frag, rest.map((p) => { const i = p.indexOf('\u0002'); return [p.slice(0, i), p.slice(i + 1)]; })];
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const cases = generate();
  const lines = ['# attribute-quirk corpus of the inline objects (js/quirk-objects.mjs, seeded): key TAB fragment [U+0001 rid U+0002 part]...'];
  for (const [k, frag, parts] of cases) lines.push(`${k}\t${esc(frag + parts.map(([rid, t]) => `\u0001${rid}\u0002${t}`).join(''))}`);
  writeFileSync(CORPUS_FILE, lines.join('\n') + '\n');
  console.log(`${cases.length} cases written to ${CORPUS_FILE}`);
}
