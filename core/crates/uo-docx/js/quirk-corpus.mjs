// The generated attribute-quirk corpus of the D1a reader (G18; frontier R1 writes it, the others
// extend it): seeded docx parts (`word/document.xml` texts) whose property elements carry the
// values real files and fuzzers put in attributes: `"1e3"`, `" 12 "`, `"12pt"`, `"0x10"`, `"-0"`,
// `"NaN"`, empty and missing attributes, duplicated children, nested `rPrChange`/`pPrChange`
// chains (also past the XML depth limit), integer-like attribute names, non-BMP text, entity
// references (surrogate ones too), unknown children and stray text.
//
//   node core/crates/uo-docx/js/quirk-corpus.mjs        writes tests/parity/props.quirks.txt
//
// The file (one case per line, `key<TAB>text` with `\`, LF, CR and TAB escaped) is committed: the
// capture script (capture-props.mjs) and the Rust replay (tests/parity_props.rs) both read it, so
// the TS and Rust sides see exactly the same bytes. `generate()` is deterministic.
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Rng } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));

export const NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" '
  + 'xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml" xmlns:w16du="http://schemas.microsoft.com/office/word/2023/wordml/word16du"';

/** A `word/document.xml` text holding `inner` under `w:body`. */
export const wrap = (inner) => `<w:document ${NS}><w:body>${inner}</w:body></w:document>`;

export const NUMS = [
  '0', '1', '-1', '12', '-12', '240', '1e3', '1E3', ' 12 ', '12 ', '\t7', '12pt', '12PT', '0x10', '-0', '+5', 'NaN', 'Infinity', '', '1.5', '1.5.2',
  '.', '-', '-.', '..5', '.5', '5.', '12mm', '12cm', '12in', '12pc', '12pi', '1.5in', '-3.7mm', '99999999999999999999', '1e400', '９', '१२', '12px',
  '12%', '50%', '5 0', '--5', '0012', '1_000', '3.7', '2.5pt', '-0pt', '0.5cm', '1e2pt', '4294967296', '-2147483649',
];
export const FLAGS = ['0', '1', 'true', 'false', 'off', 'on', 'none', '', 'TRUE', 'False', ' 0', 'yes'];
export const WORDS = ['auto', 'single', 'double', 'nil', 'none', 'left', 'right', 'center', 'start', 'end', 'both', 'dot', 'x', '', 'Calibri', '𝒜', 'ü', 'a b', 'a&amp;b', '&lt;&quot;'];
export const COLORS = ['auto', 'FF0000', 'ff00aa', '', '12345', 'red', 'ÿ0ß'];
export const HEX = ['0000', '04A0', '0x04A0', 'FFFFFFFFFFFFFFFFFFFF', '-1', 'zz', '', '  20', '1F', '07FF', '0400'];
export const TINTS = ['80', 'FF', '', 'zz'];

const esc = (s) => s.replace(/&(?!(amp|lt|quot);)/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;');

export class Gen {
  constructor(seed) { this.rng = new Rng(seed); }
  pick(a) { return a[this.rng.below(a.length)]; }
  chance(n) { return this.rng.below(n) === 0; }
  num() { return this.pick(NUMS); }
  /** ` name="value"` (or nothing, one time in four). */
  attr(name, pool) { return this.chance(4) ? '' : ` ${name}="${esc(this.pick(pool))}"`; }
  attrs(names, pool) { return names.map((n) => this.attr(n, pool)).join(''); }
  val(pool) { return this.attr('w:val', pool); }
  el(name, attrs = '', inner = '') { return inner ? `<${name}${attrs}>${inner}</${name}>` : `<${name}${attrs}/>`; }
  /** A random subset of `makers`, in random order, some repeated. */
  some(makers, min = 1, max = 8) {
    const n = min + this.rng.below(max - min + 1);
    let out = '';
    for (let i = 0; i < n; i++) out += this.pick(makers)();
    return out;
  }
  noise() {
    return this.pick(['', '', '', '<w:unknown w:val="1"/>', 'stray text', '<!-- c -->', '<w:b w:val="0"/><w:b/>', '<w:sz w:val="1"/><w:sz w:val="2"/>']);
  }

  border(name) {
    return this.el(name, this.val(WORDS) + this.attrs(['w:sz', 'w:space'], NUMS) + this.attrs(['w:color', 'w:themeColor'], COLORS)
      + this.attrs(['w:themeTint', 'w:themeShade'], TINTS) + this.attrs(['w:shadow', 'w:frame'], FLAGS));
  }
  shd(name = 'w:shd') {
    return this.el(name, this.attrs(['w:val', 'w:color', 'w:fill', 'w:themeFill', 'w:themeFillTint', 'w:themeFillShade', 'w:themeColor'], COLORS));
  }
  width(name) { return this.el(name, this.attrs(['w:w'], NUMS) + this.attr('w:type', ['dxa', 'pct', 'auto', 'nil', 'x', '', 'DXA'])); }
  fxColor() {
    const mods = () => this.some([
      () => this.el(this.pick(['w14:alpha', 'w14:lumMod', 'w14:lumOff', 'w14:tint', 'w14:shade', 'w14:satMod', 'w14:hueMod']), this.attr('w14:val', NUMS)),
      () => this.el('w14:alpha', this.attr('w14:val', NUMS)),
      () => this.el('a:alpha', this.attr('w14:val', NUMS)),
      () => this.el('ab', this.attr('w14:val', NUMS)),
    ], 0, 3);
    return this.pick([
      () => this.el('w14:srgbClr', this.attr('w14:val', ['FF0000', 'ab12cd', '', 'ß', '12']), mods()),
      () => this.el('w14:schemeClr', this.attr('w14:val', ['accent1', 'tx1', '', 'bg2']), mods()),
      () => this.el('w14:srgbClr') + this.el('w14:schemeClr'),
      () => '',
    ])();
  }
  fxFill() {
    return this.pick([
      () => this.el('w14:noFill'),
      () => this.el('w14:solidFill', '', this.fxColor()),
      () => this.el('w14:solidFill'),
      () => this.el('w14:gradFill', '', this.el('w14:gsLst', '', this.some([() => this.el('w14:gs', this.attr('w14:pos', NUMS), this.fxColor())], 0, 3))
        + this.pick([() => this.el('w14:lin', this.attr('w14:ang', NUMS) + this.attr('w14:scaled', ['1', 'true', '0', 'false', ''])), () => '',
          () => this.el('w14:path', this.attr('w14:path', ['circle', 'rect', 'shape', '']), this.pick([() => this.el('w14:fillToRect', this.attrs(['w14:l', 'w14:t', 'w14:r', 'w14:b'], NUMS)), () => ''])())])()),
      () => this.el('w14:gradFill', '', this.some([() => this.el('w14:gs', this.attr('w14:pos', NUMS), this.fxColor())], 1, 2)),
      () => '',
    ])();
  }

  rPr(depth = 0) {
    const flag = (n) => () => this.el(n, this.val(FLAGS));
    const flags = ['w:b', 'w:bCs', 'w:i', 'w:iCs', 'w:caps', 'w:smallCaps', 'w:strike', 'w:dstrike', 'w:outline', 'w:shadow', 'w:emboss', 'w:imprint', 'w:vanish', 'w:specVanish', 'w:webHidden', 'w:noProof', 'w:snapToGrid', 'w:rtl', 'w:cs'].map(flag);
    const word = (n) => () => this.el(n, this.val(WORDS));
    const numeric = (n) => () => this.el(n, this.val(NUMS));
    const makers = [
      ...flags, ...['w:rStyle', 'w:highlight', 'w:effect', 'w:em'].map(word), ...['w:spacing', 'w:w', 'w:kern', 'w:position', 'w:sz', 'w:szCs'].map(numeric),
      () => this.el('w:rFonts', this.attrs(['w:ascii', 'w:hAnsi', 'w:eastAsia', 'w:cs', 'w:asciiTheme', 'w:hAnsiTheme', 'w:eastAsiaTheme', 'w:cstheme', 'w:hint'], WORDS)),
      () => this.el('w:color', this.val(COLORS) + this.attr('w:themeColor', WORDS) + this.attrs(['w:themeTint', 'w:themeShade'], TINTS)),
      () => this.el('w:u', this.val(WORDS) + this.attr('w:color', COLORS) + this.attr('w:themeColor', WORDS) + this.attrs(['w:themeTint', 'w:themeShade'], TINTS)),
      () => this.border('w:bdr'),
      () => this.shd(),
      () => this.el('w:vertAlign', this.val(['superscript', 'subscript', 'baseline', 'Superscript', '', 'x'])),
      () => this.el('w:lang', this.attrs(['w:val', 'w:eastAsia', 'w:bidi'], ['pt-BR', 'en', '', 'ja-JP'])),
      () => this.el('w14:textFill', '', this.pick([() => this.el('w14:solidFill', '', this.pick([
        () => this.el('w14:srgbClr', this.attr('w14:val', ['FF0000', 'ab12cd', '', 'ß']), this.pick([() => this.el('w14:alpha', this.attr('w14:val', NUMS)), () => ''])()),
        () => this.el('w14:schemeClr', this.attr('w14:val', ['accent1', '', 'tx1']), this.pick([() => this.el('w14:alpha', this.attr('w14:val', NUMS)), () => ''])()),
        () => '',
      ])()), () => this.el('w14:noFill'), () => '', () => this.fxFill()])()),
      () => this.el('w14:ligatures', this.attr('w14:val', ['none', 'standard', 'all', '', 'contextual'])),
      () => this.el('w14:shadow', this.attrs(['w14:blurRad', 'w14:dist', 'w14:dir', 'w14:sx', 'w14:sy', 'w14:kx', 'w14:ky'], NUMS) + this.attr('w14:algn', ['b', 'tl', '']), this.fxColor()),
      () => this.el('w14:glow', this.attr('w14:rad', NUMS), this.fxColor()),
      () => this.el('w14:reflection', this.attrs(['w14:blurRad', 'w14:stA', 'w14:stPos', 'w14:endA', 'w14:endPos', 'w14:dist', 'w14:dir', 'w14:fadeDir', 'w14:sx', 'w14:sy', 'w14:kx', 'w14:ky'], NUMS) + this.attr('w14:algn', ['b', 'ctr', ''])),
      () => this.el('w14:textOutline', this.attr('w14:w', NUMS) + this.attr('w14:cap', ['rnd', 'sq', 'flat', '']) + this.attr('w14:cmpd', ['sng', 'dbl', '']) + this.attr('w14:algn', ['ctr', 'in', '']),
        this.fxFill() + this.pick([() => this.el('w14:prstDash', this.attr('w14:val', ['dash', 'sysDot', ''])), () => ''])()
        + this.pick([() => this.el('w14:round'), () => this.el('w14:bevel'), () => this.el('w14:miter', this.attr('w14:lim', NUMS)), () => this.el('w14:round') + this.el('w14:bevel'), () => ''])()),
      () => this.el('w14:scene3d', this.attr('w14:x', ['a&amp;b', 'q"q']), this.el('w14:camera', this.attr('w14:prst', ['orthographicFront', ''])) + this.el('w14:lightRig', this.attrs(['w14:rig', 'w14:dir'], WORDS))),
      () => this.el('w14:props3d', this.attr('w14:extrusionH', NUMS)),
      () => this.el('w14:numForm', this.attr('w14:val', ['lining', 'oldStyle', 'default', '', 'x'])),
      () => this.el('w14:numSpacing', this.attr('w14:val', ['proportional', 'tabular', '', 'x'])),
      () => this.el('w14:stylisticSets', '', this.some([() => this.el('w14:styleSet', this.attr('w14:id', ['1', '20', '0', '21', '5', '', 'x', '3.5', '-2', '1e1']))], 0, 4)),
      () => this.el('w14:cntxtAlts', this.attr('w14:val', ['0', 'false', '1', 'true', '', 'off'])),
      () => this.noise(),
    ];
    if (depth < 3) {
      makers.push(
        () => this.el('w:rPrChange', this.attr('w:author', WORDS) + this.attr('w:date', ['2024-01-02T03:04:05Z', '', 'x']) + this.attr('w16du:dateUtc', ['2024-01-02T03:04:05Z', '', 'y']) + this.attr('w:id', ['1']),
          this.pick([() => this.el('w:rPr', '', this.rPr(depth + 1)), () => '', () => this.el('w:rPr')])()),
      );
    }
    return this.some(makers, 0, 9);
  }

  pPr(depth = 0) {
    const flag = (n) => () => this.el(n, this.val(FLAGS));
    const flags = ['w:keepNext', 'w:keepLines', 'w:pageBreakBefore', 'w:widowControl', 'w:suppressLineNumbers', 'w:suppressAutoHyphens', 'w:kinsoku', 'w:wordWrap', 'w:overflowPunct', 'w:autoSpaceDE', 'w:autoSpaceDN', 'w:bidi', 'w:adjustRightInd', 'w:snapToGrid', 'w:contextualSpacing', 'w:mirrorIndents', 'w:suppressOverlap'].map(flag);
    const word = (n) => () => this.el(n, this.val(WORDS));
    const makers = [
      ...flags, ...['w:pStyle', 'w:jc', 'w:textboxTightWrap', 'w:textAlignment'].map(word),
      () => this.el('w:outlineLvl', this.val(NUMS)),
      () => this.el('w:numPr', '', this.some([() => this.el('w:numId', this.val(NUMS)), () => this.el('w:ilvl', this.val(NUMS)), () => this.noise()], 0, 3)),
      () => this.el('w:pBdr', '', this.some(['w:top', 'w:left', 'w:start', 'w:bottom', 'w:right', 'w:end', 'w:between', 'w:bar', 'w:insideH'].map((n) => () => this.border(n)), 0, 6)),
      () => this.shd(),
      () => this.el('w:tabs', '', this.some([() => this.el('w:tab', this.attr('w:val', ['left', 'center', 'right', 'decimal', 'bar', 'clear', 'num', 'start', 'end', 'x', '']) + this.attr('w:pos', NUMS) + this.attr('w:leader', ['dot', 'none', 'hyphen', 'underscore', 'heavy', 'middleDot', 'x', '']))
        , () => this.noise()], 0, 5)),
      () => this.el('w:spacing', this.attrs(['w:before', 'w:after', 'w:beforeLines', 'w:afterLines', 'w:line'], NUMS) + this.attrs(['w:beforeAutospacing', 'w:afterAutospacing'], FLAGS) + this.attr('w:lineRule', ['auto', 'exact', 'atLeast', 'x', ''])),
      () => this.el('w:ind', this.attrs(['w:left', 'w:start', 'w:right', 'w:end', 'w:hanging', 'w:firstLine'], NUMS) + this.attrs(['w:leftChars', 'w:startChars', 'w:rightChars', 'w:endChars', 'w:hangingChars', 'w:firstLineChars'], NUMS)),
      () => this.el('w:framePr', this.attr('w:dropCap', ['none', 'drop', 'margin', 'x', '']) + this.attrs(['w:lines', 'w:w', 'w:h', 'w:hSpace', 'w:vSpace', 'w:x', 'w:y'], NUMS)
        + this.attr('w:hRule', ['auto', 'exact', 'atLeast', 'x', '']) + this.attrs(['w:wrap', 'w:hAnchor', 'w:vAnchor', 'w:xAlign', 'w:yAlign'], WORDS) + this.attr('w:anchorLock', FLAGS)),
      () => this.el('w:rPr', '', this.rPr(depth) + this.some([
        () => this.el(this.pick(['w:ins', 'w:del', 'w:moveFrom', 'w:moveTo']), this.attr('w:author', WORDS) + this.attr('w:date', ['2024-01-02T03:04:05Z', '']) + this.attr('w16du:dateUtc', ['2024-01-02T03:04:05Z', '']) + this.attr('w:id', ['1'])),
        () => '',
      ], 0, 3)),
      () => this.noise(),
    ];
    if (depth < 3) {
      makers.push(() => this.el('w:pPrChange', this.attr('w:author', WORDS) + this.attr('w:date', ['2024-01-02T03:04:05Z', '']) + this.attr('w16du:dateUtc', ['x', '']) + this.attr('w:id', ['2']),
        this.pick([() => this.el('w:pPr', '', this.pPr(depth + 1)), () => '', () => this.el('w:pPr')])()));
    }
    return this.some(makers, 0, 9);
  }

  tblBorders(name) {
    return this.el(name, '', this.some(['w:top', 'w:left', 'w:start', 'w:bottom', 'w:right', 'w:end', 'w:insideH', 'w:insideV', 'w:tl2br', 'w:tr2bl'].map((n) => () => this.border(n)), 0, 7));
  }
  cellMar(name) {
    return this.el(name, '', this.some(['w:top', 'w:left', 'w:start', 'w:bottom', 'w:right', 'w:end'].map((n) => () => this.width(n)), 0, 5));
  }

  tblPr() {
    const makers = [
      () => this.el('w:tblStyle', this.val(WORDS)),
      () => this.el('w:tblpPr', this.attrs(['w:leftFromText', 'w:rightFromText', 'w:topFromText', 'w:bottomFromText', 'w:tblpX', 'w:tblpY'], NUMS)
        + this.attrs(['w:vertAnchor', 'w:horzAnchor'], ['text', 'margin', 'page', 'x', '']) + this.attrs(['w:tblpXSpec', 'w:tblpYSpec'], WORDS)),
      () => this.el('w:tblOverlap', this.val(WORDS)),
      () => this.el('w:bidiVisual', this.val(FLAGS)),
      () => this.el('w:tblStyleRowBandSize', this.val(NUMS)),
      () => this.el('w:tblStyleColBandSize', this.val(NUMS)),
      () => this.width('w:tblW'), () => this.width('w:tblCellSpacing'), () => this.width('w:tblInd'),
      () => this.el('w:jc', this.val(WORDS)),
      () => this.tblBorders('w:tblBorders'),
      () => this.shd(),
      () => this.el('w:tblLayout', this.attr('w:type', ['fixed', 'autofit', 'x', ''])),
      () => this.el('w:tblCaption', this.val(WORDS)), () => this.el('w:tblDescription', this.val(WORDS)),
      () => this.cellMar('w:tblCellMar'),
      () => this.el('w:tblLook', this.attr('w:val', HEX) + this.attrs(['w:firstRow', 'w:lastRow', 'w:firstColumn', 'w:lastColumn', 'w:noHBand', 'w:noVBand'], FLAGS)),
      () => this.noise(),
    ];
    return this.some(makers, 0, 9);
  }

  trPr() {
    return this.some([
      () => this.el('w:cnfStyle', this.val(['100000000000', '', '0'])),
      () => this.el('w:gridBefore', this.val(NUMS)), () => this.el('w:gridAfter', this.val(NUMS)),
      () => this.width('w:wBefore'), () => this.width('w:wAfter'), () => this.width('w:tblCellSpacing'),
      () => this.el('w:cantSplit', this.val(FLAGS)), () => this.el('w:tblHeader', this.val(FLAGS)), () => this.el('w:hidden', this.val(FLAGS)),
      () => this.el('w:trHeight', this.val(NUMS) + this.attr('w:hRule', ['auto', 'exact', 'atLeast', 'x', ''])),
      () => this.el('w:jc', this.val(WORDS)),
      () => this.noise(),
    ], 0, 7);
  }

  tcPr() {
    return this.some([
      () => this.el('w:cnfStyle', this.val(['100000000000', '', '0'])),
      () => this.width('w:tcW'),
      () => this.el('w:gridSpan', this.val(NUMS)),
      () => this.el('w:hMerge', this.val(['restart', 'continue', '', 'x'])), () => this.el('w:vMerge', this.val(['restart', 'continue', '', 'x'])),
      () => this.tblBorders('w:tcBorders'),
      () => this.shd(),
      () => this.el('w:noWrap', this.val(FLAGS)), () => this.el('w:tcFitText', this.val(FLAGS)), () => this.el('w:hideMark', this.val(FLAGS)),
      () => this.cellMar('w:tcMar'),
      () => this.el('w:textDirection', this.val(['btLr', 'lrTb', '', 'x'])),
      () => this.el('w:vAlign', this.val(['top', 'center', 'bottom', 'both', 'x', ''])),
      () => this.noise(),
    ], 0, 7);
  }

  /** One random property element wrapped in an element it could sit in. */
  property() {
    const kind = this.pick(['rPr', 'rPr', 'pPr', 'pPr', 'tblPr', 'tblPrEx', 'trPr', 'tcPr']);
    switch (kind) {
      case 'rPr': return this.chance(3) ? `<w:r><w:rPr>${this.rPr()}</w:rPr><w:t>x</w:t></w:r>` : `<w:rPr>${this.rPr()}</w:rPr>`;
      case 'pPr': return `<w:p><w:pPr>${this.pPr()}</w:pPr></w:p>`;
      case 'tblPr': return `<w:tbl><w:tblPr>${this.tblPr()}</w:tblPr></w:tbl>`;
      case 'tblPrEx': return `<w:tr><w:tblPrEx>${this.tblPr()}</w:tblPrEx></w:tr>`;
      case 'trPr': return `<w:tr><w:trPr>${this.trPr()}</w:trPr></w:tr>`;
      default: return `<w:tc><w:tcPr>${this.tcPr()}</w:tcPr></w:tc>`;
    }
  }
}

/** A chain `w:rPrChange > w:rPr > w:rPrChange …` of `n` levels (each level also carries a `w:b`). */
const rChain = (n) => {
  let x = '<w:b w:val="0"/>';
  for (let i = 0; i < n; i++) x = `<w:b w:val="${i % 2}"/><w:rPrChange w:author="a${i}"><w:rPr>${x}</w:rPr></w:rPrChange>`;
  return `<w:rPr>${x}</w:rPr>`;
};
const pChain = (n) => {
  let x = '<w:jc w:val="left"/>';
  for (let i = 0; i < n; i++) x = `<w:jc w:val="c${i}"/><w:pPrChange w:author="a${i}"><w:pPr>${x}</w:pPr></w:pPrChange>`;
  return `<w:p><w:pPr>${x}</w:pPr></w:p>`;
};

/** The hand-written cases (one per named quirk of the reader's own rules). */
export function fixed() {
  const c = [];
  const add = (key, inner) => c.push([key, wrap(inner)]);
  add('measure', ['', ' ', '1', '12pt', ' 12pt ', '12 pt', '1e3', '0x10', '-0', '-0pt', '.', '-.', '..5', '1.2.3', '1.2.3mm', 'NaN', '１２', '12PT', '2.5pc', '7pi', '25.4mm', '2.54cm', '1.5in', '99999999999999999999pt', '12 ', '\u000b12\u000b', '12pt\n']
    .map((v) => `<w:pPr><w:spacing w:before="${v}" w:after="${v}" w:line="${v}"/><w:ind w:left="${v}" w:hanging="${v}"/></w:pPr>`).join(''));
  add('int', ['', ' 7 ', '7.9', '-7.9', '1e3', '0x10', '-0', '+3', 'abc', '12abc', '99999999999999999999', '1e400', '９', '  7'].map((v) => `<w:rPr><w:sz w:val="${v}"/><w:kern w:val="${v}"/><w:position w:val="${v}"/><w:w w:val="${v}%"/></w:rPr>`).join(''));
  add('width', ['5000', '50%', '12.5%', 'x%', '%', '', '12pt', '1e3', 'abc', '-0'].flatMap((w) => ['dxa', 'pct', 'auto', 'nil', 'x', ''].map((t) => `<w:tblPr><w:tblW w:w="${w}" w:type="${t}"/><w:tblInd w:w="${w}"/></w:tblPr>`)).join(''));
  add('look', HEX.map((h) => `<w:tblPr><w:tblLook w:val="${h}" w:firstRow="1" w:noVBand="off"/><w:tblLook w:val="${h}"/></w:tblPr>`).join(''));
  add('onoff', FLAGS.map((f) => `<w:rPr><w:b w:val="${f}"/><w:i/></w:rPr><w:pPr><w:keepNext w:val="${f}"/></w:pPr><w:rPr><w:bdr w:val="x" w:shadow="${f}" w:frame="${f}"/></w:rPr>`).join(''));
  add('textfill', [
    '<w14:textFill><w14:solidFill><w14:srgbClr w14:val="ab12ef"><w14:alpha w14:val="25000"/></w14:srgbClr></w14:solidFill></w14:textFill>',
    '<w14:textFill><w14:solidFill><w14:srgbClr w14:val=""><w14:alpha w14:val="abc"/></w14:srgbClr></w14:solidFill></w14:textFill>',
    '<w14:textFill><w14:solidFill><w14:schemeClr w14:val="accent2"><w14:alpha w14:val="200000"/></w14:schemeClr></w14:solidFill></w14:textFill>',
    '<w14:textFill><w14:solidFill><w14:srgbClr/><w14:schemeClr w14:val="x"/></w14:solidFill></w14:textFill>',
    '<w14:textFill><w14:solidFill/></w14:textFill>', '<w14:textFill><w14:noFill/></w14:textFill>', '<w14:textFill/>',
    '<w14:textFill><w14:solidFill><w14:srgbClr w14:val="ß"/></w14:solidFill></w14:textFill>',
  ].map((x) => `<w:rPr>${x}</w:rPr>`).join(''));
  add('shadow14', ['0', '12700', '-12700', '1e3', 'abc', '', '12700.9', ' 5 '].map((v) => `<w:rPr><w14:shadow w14:blurRad="${v}" w14:dist="${v}" w14:dir="${v}"><w14:srgbClr w14:val="000000"/></w14:shadow></w:rPr>`).join(''));
  add('order', '<w:rPr><w:sz w:val="4"/><w:b/><w:color w:val="FF0000"/><w:sz w:val="6"/><w14:ligatures w14:val="all"/><w:b w:val="0"/></w:rPr>'
    + '<w:pPr><w:jc w:val="left"/><w:spacing w:after="5" w:before="6" w:line="7" w:lineRule="auto" w:beforeLines="1" w:afterAutospacing="1"/><w:keepNext/><w:jc w:val="right"/></w:pPr>'
    + '<w:pPr><w:framePr w:y="1" w:x="2" w:hRule="exact" w:wrap="around" w:lines="3" w:dropCap="drop" w:anchorLock="1"/></w:pPr>'
    + '<w:tblPr><w:tblpPr w:tblpY="1" w:vertAnchor="page" w:tblpX="2" w:tblpXSpec="left" w:leftFromText="3"/><w:tblW w:w="1"/><w:jc w:val="left"/></w:tblPr>'
    + '<w:tcPr><w:vAlign w:val="top"/><w:gridSpan w:val="2"/><w:cnfStyle w:val="1"/></w:tcPr><w:trPr><w:jc w:val="center"/><w:cantSplit/><w:cnfStyle w:val="2"/></w:trPr>');
  add('fx', '<w:rPr><w14:scene3d><w14:camera w14:prst="orthographicFront"/><w14:lightRig w14:rig="threePt" w14:dir="t"/></w14:scene3d><w14:props3d w14:extrusionH="12700"><w14:bevelT w14:w="1"/></w14:props3d>'
    + '<w14:textOutline w14:w="9525" w14:cap="flat" w14:cmpd="sng" w14:algn="ctr"><w14:solidFill><w14:srgbClr w14:val="ff00ff"><w14:lumMod w14:val="75000"/><w14:shade w14:val="5"/></w14:srgbClr></w14:solidFill><w14:prstDash w14:val="solid"/><w14:miter w14:lim="800000"/></w14:textOutline>'
    + '<w14:stylisticSets><w14:styleSet w14:id="1"/><w14:styleSet w14:id="0"/><w14:styleSet w14:id="20"/><w14:styleSet w14:id="21"/><w14:styleSet/></w14:stylisticSets></w:rPr>');
  add('scene3d-text', '<w:rPr><w14:scene3d a="1" b="&amp;&lt;&gt;&quot;">t&amp;x<w14:e/>&lt;tail<![CDATA[<c>]]><w14:f g="h"></w14:f><w14:i> </w14:i></w14:scene3d></w:rPr>'
    + '<w:rPr><w14:scene3d></w14:scene3d><w14:props3d>x</w14:props3d></w:rPr><w:rPr><w14:scene3d><w14:a><w14:b><w14:c/></w14:b></w14:a></w14:scene3d></w:rPr>');
  add('scene3d-intkeys', '<w:rPr><w14:scene3d z="3" 10="a" 2="b" y="4" 1="c"><w14:x 5="x" a="y" 0="z"/></w14:scene3d></w:rPr>');
  add('scene3d-nonbmp', '<w:rPr><w14:scene3d x="𝒜" ü="é">𝒜&#x1D49C;&#128512;&#xFFFD;<w14:𝒜x/></w14:scene3d></w:rPr>');
  add('surrogate-refs', '<w:rPr><w:rStyle w:val="a&#xD800;b"/><w14:scene3d x="&#xDC00;&#xD83D;&#xDE00;">&#xD800;</w14:scene3d></w:rPr>');
  add('mod-names', '<w:rPr><w14:glow w14:rad="1"><w14:srgbClr w14:val="aa"><w14:ab/><w14:abcd w14:val="5"/><a:x w14:val="6"/><w14:alpha w14:val="7"/><w14:𝒜𝒜 w14:val="8"/></w14:srgbClr></w14:glow></w:rPr>');
  add('mod-name-cut', '<w:rPr><w14:glow w14:rad="1"><w14:srgbClr w14:val="aa"><abc𝒜 w14:val="5"/></w14:srgbClr></w14:glow></w:rPr>');
  add('dup-children', '<w:rPr><w:rFonts w:ascii="a"/><w:rFonts w:hAnsi="b"/><w:u/><w:u w:val="x" w:color="c"/><w:bdr/><w:bdr w:val="x"/></w:rPr><w:pPr><w:ind w:left="1"/><w:ind w:right="2"/><w:tabs><w:tab w:val="left" w:pos="1"/></w:tabs><w:tabs/></w:pPr>');
  add('dup-attrs', '<w:rPr><w:b w:val="0" w:val="1"/><w:sz w:val="3" w:val="x"/></w:rPr>');
  add('chain-2', rChain(2) + pChain(2));
  add('chain-70', rChain(70) + pChain(70));
  add('chain-400', rChain(400) + pChain(400));
  add('chain-600', rChain(600));
  add('chain-480', rChain(480));
  add('multi-change', '<w:rPr><w:b/><w:rPrChange w:author="first"><w:rPr><w:i/></w:rPr></w:rPrChange><w:sz w:val="2"/><w:rPrChange w:author="last" w:date="d"><w:rPr><w:u/></w:rPr></w:rPrChange></w:rPr>'
    + '<w:pPr><w:jc w:val="left"/><w:pPrChange w:author="first"><w:pPr><w:keepNext/></w:pPr></w:pPrChange><w:spacing w:after="1"/><w:pPrChange w:author="last"/></w:pPr>'
    + '<w:rPr><w:rPrChange w:author="x"><w:b/></w:rPrChange></w:rPr><w:rPr><w:rPrChange w:author="y"><w:rPr><w:rPrChange w:author="z"/></w:rPr></w:rPrChange></w:rPr>');
  add('mark-rpr', '<w:pPr><w:rPr><w:ins w:author="i"/><w:del w:author="d" w:date="dd"/><w:b/></w:rPr><w:rPr><w:moveFrom w:author="mf" w16du:dateUtc="u"/><w:moveTo w:author="mt"/></w:rPr></w:pPr>'
    + '<w:pPr><w:rPr><w:ins/></w:rPr></w:pPr><w:pPr><w:rPr><w:b/><w:del/><w:rPrChange w:author="rc"/></w:rPr></w:pPr>');
  add('borders', '<w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4"/><w:start w:val="x"/><w:left w:val="y"/><w:end w:val="z"/><w:insideH/><w:tl2br w:val="a"/><w:tr2bl/></w:tblBorders></w:tblPr>'
    + '<w:tcPr><w:tcBorders><w:tl2br w:val="a" w:sz="x"/><w:start w:val="b"/><w:right w:val="r"/><w:end/><w:top/></w:tcBorders></w:tcPr>'
    + '<w:pPr><w:pBdr><w:start w:val="s"/><w:end w:val="e"/><w:left w:val="l"/><w:bar w:val="b"/><w:between/></w:pBdr></w:pPr>');
  add('empty', '<w:rPr/><w:pPr/><w:tblPr/><w:tblPrEx/><w:trPr/><w:tcPr/><w:rPr></w:rPr><w:rPr> </w:rPr><w:rPr>text</w:rPr>');
  add('unknown', '<w:rPr><w:foo/><x:bar xmlns:x="urn:x"/>text<w:b/><!-- c --></w:rPr>');
  add('nested-props', '<w:pPr><w:rPr><w:rPr><w:b/></w:rPr></w:rPr><w:pPr><w:jc w:val="x"/></w:pPr></w:pPr><w:rPr><w:pPr><w:jc w:val="y"/></w:pPr></w:rPr>');
  add('tabs', '<w:pPr><w:tabs><w:tab w:val="start" w:pos="100"/><w:tab w:pos="200"/><w:tab w:val="end" w:pos="300" w:leader="dot"/><w:tab w:val="bogus" w:pos="1"/><w:tab w:val="left"/><w:tab w:val="left" w:pos="x"/>'
    + '<w:tab w:val="clear" w:pos="1e3" w:leader="x"/><w:tab w:val="num" w:pos="-5pt" w:leader="middleDot"/><w:foo/></w:tabs></w:pPr>');
  // deeper than the XML limit: both readers must refuse the part
  c.push(['too-deep', `<w:document ${NS}>${'<w:x>'.repeat(1001)}<w:rPr><w:b/></w:rPr>${'</w:x>'.repeat(1001)}</w:document>`]);
  c.push(['deep-ok', `<w:document ${NS}>${'<w:x>'.repeat(900)}<w:rPr><w:b/></w:rPr>${'</w:x>'.repeat(900)}</w:document>`]);
  c.push(['not-xml', '<w:document><w:rPr>']);
  return c;
}

/** The whole corpus: `[key, text]`, hand-written cases first, then `n` seeded ones. */
export function generate(n = 600, seed = 0x5eed0a11) {
  const out = fixed().map(([k, t]) => [`quirk/${k}`, t]);
  const g = new Gen(seed);
  for (let i = 0; i < n; i++) {
    const parts = 1 + g.rng.below(4);
    let inner = '';
    for (let k = 0; k < parts; k++) inner += g.property();
    out.push([`quirk/gen-${String(i).padStart(4, '0')}`, wrap(inner)]);
  }
  return out;
}

const escLine = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
const unescLine = (x) => x.replace(/\\(.)/gs, (_, c) => (c === 'n' ? '\n' : c === 'r' ? '\r' : c === 't' ? '\t' : c));

export const CORPUS_FILE = join(here, '..', 'tests', 'parity', 'props.quirks.txt');

/** The committed corpus (`[key, text]`). */
export function readCorpus(file = CORPUS_FILE) {
  return readFileLines(file).map((l) => { const i = l.indexOf('\t'); return [l.slice(0, i), unescLine(l.slice(i + 1))]; });
}

const readFileLines = (f) => readFileSync(f, 'utf8').split('\n').filter((l) => l && !l.startsWith('#'));

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const cases = generate();
  const lines = ['# attribute-quirk corpus of the D1a reader (js/quirk-corpus.mjs, seeded): key TAB text with \, LF, CR, TAB escaped', ...cases.map(([k, t]) => `${k}\t${escLine(t)}`)];
  writeFileSync(CORPUS_FILE, lines.join('\n') + '\n');
  console.log(`${cases.length} cases written to ${CORPUS_FILE}`);
}
