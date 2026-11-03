// Seeded generator of the formatting parts of a docx (frontier R2): styles, numbering, theme,
// settings, font table and the relationships of the main part, with the attribute quirks of the
// brief (G18): "1e3", " 12 ", "12pt", "0x10", "-0", "NaN", empty and missing attributes,
// integer-like and `Object.prototype` keys, duplicate ids, `basedOn` cycles and long chains,
// non-BMP text and surrogate character references. `capture-parts.mjs` and `capture-styles.mjs`
// run the frozen TS over these inputs and write them next to the captures
// (tests/parity/parts.inputs); the Rust replays read that file, so both sides see the same text.
import { Rng } from '../../uo-testkit/js/testkit.mjs';

const W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"';
const A = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"';
const R = 'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"';
const V = 'xmlns:v="urn:schemas-microsoft-com:vml"';

class Gen {
  constructor(seed) { this.rng = new Rng(seed); }
  below(n) { return this.rng.below(n); }
  pick(a) { return a[this.below(a.length)]; }
  chance(n) { return this.below(n) === 0; }
  opt(s, n = 3) { return this.chance(n) ? '' : s; }
  count(max) { return this.below(max + 1); }
  many(max, f) { let s = ''; for (let i = this.count(max); i > 0; i--) s += f(); return s; }

  num() {
    return this.pick(['0', '1', '4', '12', '-3', '240', '720', '1e3', ' 12 ', '12pt', '0x10', '-0', 'NaN', '', '.', '1.5', '99999999999999999999', 'abc', '+7', '3 4', '١٢']);
  }
  onoff() { return this.pick(['', ' w:val="0"', ' w:val="false"', ' w:val="none"', ' w:val="off"', ' w:val="1"', ' w:val="true"', ' w:val=""', ' w:val="x"']); }
  text() { return this.pick(['Normal', 'Heading1', 'a', 'B', 'x y', '', 'Ünï', '😀', '&#xD800;', '&#x1F600;', '&amp;&lt;', 'Title']); }
  id() { return this.pick(['Normal', 'Heading1', 'a', 'B', '5', '10', 'Dup', 'Dup', '__proto__', 'constructor', 'toString', 'x-1', 'Table1', 'Lista', 'Estilo2', '0', '01', '4294967295']); }

  attr(name, v) { return v === undefined ? '' : ` ${name}="${v}"`; }

  color() { return this.pick(['FF0000', 'auto', '', '00ff00', 'zz', 'abc']); }

  rPr() {
    const kids = this.many(7, () => this.pick([
      () => `<w:b${this.onoff()}/>`, () => `<w:i${this.onoff()}/>`, () => `<w:caps${this.onoff()}/>`, () => `<w:smallCaps${this.onoff()}/>`,
      () => `<w:strike${this.onoff()}/>`, () => `<w:sz w:val="${this.num()}"/>`, () => `<w:sz/>`, () => `<w:color w:val="${this.color()}"/>`,
      () => `<w:color/>`, () => `<w:rFonts w:ascii="${this.pick(['Calibri', '', 'Aptos'])}"/>`, () => `<w:rFonts w:hAnsi="X"/>`,
      () => `<w:u w:val="${this.pick(['none', 'single', 'double', ''])}"/>`, () => `<w:u/>`, () => `<w:vertAlign w:val="superscript"/>`,
      () => `<w:lang w:val="pt-BR"/>`, () => `<w:szCs w:val="24"/>`,
    ])());
    return this.opt(`<w:rPr>${kids}</w:rPr>`, 4);
  }

  edge(tag) {
    return `<w:${tag}${this.attr('w:val', this.pick(['single', 'none', 'nil', '', 'double', undefined]))}${this.attr('w:sz', this.chance(2) ? this.num() : undefined)}${this.attr('w:color', this.chance(2) ? this.color() : undefined)}${this.attr('w:space', this.chance(2) ? this.num() : undefined)}/>`;
  }

  pPr() {
    const kids = this.many(7, () => this.pick([
      () => `<w:jc w:val="${this.pick(['center', 'right', 'both', 'left', 'start', ''])}"/>`,
      () => `<w:spacing${this.attr('w:line', this.chance(2) ? this.num() : undefined)}${this.attr('w:lineRule', this.pick(['auto', 'exact', undefined]))}${this.attr('w:after', this.chance(2) ? this.num() : undefined)}${this.attr('w:before', this.chance(2) ? this.num() : undefined)}/>`,
      () => `<w:ind${this.attr('w:left', this.chance(2) ? this.num() : undefined)}${this.attr('w:right', this.chance(2) ? this.num() : undefined)}/>`,
      () => '<w:pageBreakBefore/>', () => '<w:pageBreakBefore w:val="0"/>',
      () => `<w:pBdr>${this.many(5, () => this.edge(this.pick(['top', 'left', 'bottom', 'right', 'between', 'bar'])))}</w:pBdr>`,
      () => `<w:shd${this.attr('w:fill', this.pick(['FFFF00', 'auto', '', undefined]))}/>`,
      () => `<w:keepNext/>`, () => `<w:outlineLvl w:val="${this.num()}"/>`,
    ])());
    return this.opt(`<w:pPr>${kids}</w:pPr>`, 4);
  }

  tblPr() { return this.opt(`<w:tblPr><w:tblW w:w="${this.num()}" w:type="dxa"/></w:tblPr>`, 3); }

  style(ids) {
    const type = this.pick(['paragraph', 'paragraph', 'character', 'table', 'numbering', undefined, '', 'bogus']);
    const id = this.chance(12) ? undefined : this.pick(ids);
    const def = this.pick([undefined, undefined, '1', 'true', 'on', '0', 'false', '']);
    const val = (n, pool) => this.opt(`<w:${n}${this.attr('w:val', this.pick(pool))}/>`, 4);
    const kids = val('name', ['Normal', 'heading 1', '', undefined, 'Table Grid', 'table grid', 'Normal Table'])
      + val('basedOn', [...ids, '', undefined, 'nope'])
      + val('next', ids) + val('link', ids)
      + this.pPr() + this.rPr() + this.tblPr()
      + this.opt('<w:trPr><w:cantSplit/></w:trPr>', 4) + this.opt('<w:tcPr><w:tcW w:w="100" w:type="dxa"/></w:tcPr>', 4)
      + this.many(3, () => `<w:tblStylePr${this.attr('w:type', this.pick(['firstRow', 'lastRow', 'band1Horz', '__proto__', '', undefined, 'wholeTable']))}>${this.pPr()}${this.rPr()}${this.tblPr()}</w:tblStylePr>`);
    return `<w:style${this.attr('w:type', type)}${this.attr('w:styleId', id)}${this.attr('w:default', def)}>${kids}</w:style>`;
  }

  styles() {
    const ids = [this.id(), this.id(), this.id(), this.id(), 'Normal'];
    let body = '';
    if (this.chance(2)) {
      body += `<w:docDefaults>${this.opt(`<w:rPrDefault>${this.rPr()}</w:rPrDefault>`, 3)}${this.opt(`<w:pPrDefault>${this.pPr()}</w:pPrDefault>`, 3)}${this.chance(6) ? this.rPr() + this.pPr() : ''}</w:docDefaults>`;
    }
    body += this.many(10, () => this.style(ids));
    if (this.chance(5)) {
      // a chain and cycles
      const n = 2 + this.below(14);
      for (let i = 0; i < n; i++) body += `<w:style w:type="paragraph" w:styleId="C${i}"><w:name w:val="c${i}"/><w:basedOn w:val="C${(i + 1) % (this.chance(3) ? n : n + 5)}"/>${this.rPr()}${this.pPr()}</w:style>`;
    }
    if (this.chance(8)) body += '<w:style w:type="paragraph" w:styleId="docDefaults"><w:name w:val="docDefaults"/></w:style>';
    return `<?xml version="1.0"?><w:styles ${W}>${this.opt(body, 12)}</w:styles>`;
  }

  lvl(ilvl) {
    return `<w:lvl${this.attr('w:ilvl', ilvl)}>${this.opt(`<w:start${this.attr('w:val', this.num())}/>`)}${this.opt(`<w:numFmt${this.attr('w:val', this.pick(['decimal', 'bullet', '', undefined]))}${this.attr('w:format', this.pick(['001, 002', '', undefined]))}/>`)}`
      + this.opt(`<w:lvlRestart${this.attr('w:val', this.num())}/>`, 4) + this.opt(`<w:pStyle${this.attr('w:val', this.pick(['Heading1', '', undefined]))}/>`, 4)
      + this.opt(`<w:isLgl${this.onoff()}/>`, 4) + this.opt(`<w:suff${this.attr('w:val', this.pick(['tab', 'space', 'nothing', 'x', '']))}/>`, 4)
      + this.opt(`<w:lvlText${this.attr('w:val', this.pick(['%1.', '', '•', undefined]))}${this.attr('w:null', this.pick(['1', 'true', '0', undefined, undefined]))}/>`)
      + this.opt(`<w:lvlPicBulletId${this.attr('w:val', this.num())}/>`, 4) + this.opt(`<w:lvlJc${this.attr('w:val', this.pick(['left', '', undefined]))}/>`, 4)
      + this.pPr() + this.rPr() + '</w:lvl>';
  }

  numbering() {
    let body = this.many(3, () => {
      const imagedata = this.opt(`<v:imagedata${this.attr('r:id', this.pick(['rId1', 'rId2', '', undefined]))}/>`);
      const style = this.attr('style', this.pick(['width:10pt;height:5in', ' ; width : 3', 'WIDTH:3', 'width:.5cm;height:2mm', 'width:1.2.3mm', 'width:abc', 'width:1e3px', ';width:4pc;;height:6', 'height:7 in', 'width:8;width:9pt', '', undefined]));
      const shape = this.opt(`<v:shape${style}>${this.chance(3) ? '' : imagedata}</v:shape>`, 3);
      const blip = this.opt(`<a:blip${this.attr('r:embed', this.pick(['rId1', 'rId3', '', undefined]))}/>`, 4);
      return `<w:numPicBullet${this.attr('w:numPicBulletId', this.pick(['0', '1', '2', '-1', 'x', '', undefined, '1e3']))}><w:pict>${shape}${this.chance(3) ? imagedata : ''}</w:pict>${blip}</w:numPicBullet>`;
    });
    body += this.many(4, () => {
      let lv = '';
      for (let i = this.count(this.chance(15) ? 12 : 4); i > 0; i--) lv += this.lvl(this.pick(['0', '1', '2', '2', '-1', '1e3', 'x', '', undefined, '99999999999999999999999']));
      return `<w:abstractNum${this.attr('w:abstractNumId', this.pick(['0', '1', '2', '-0', 'NaN', '', undefined, '1e3', '3']))}>${this.opt(`<w:multiLevelType${this.attr('w:val', this.pick(['multilevel', '', undefined]))}/>`)}${this.opt(`<w:styleLink${this.attr('w:val', this.pick(['L1', '', undefined]))}/>`, 4)}${this.opt(`<w:numStyleLink${this.attr('w:val', this.pick(['L2', '', undefined]))}/>`, 4)}${lv}</w:abstractNum>`;
    });
    body += this.many(4, () => `<w:num${this.attr('w:numId', this.pick(['1', '2', '3', '-0', 'q', '', undefined]))}>${this.opt(`<w:abstractNumId${this.attr('w:val', this.pick(['0', '1', 'x', '', undefined]))}/>`)}${this.many(3, () => `<w:lvlOverride${this.attr('w:ilvl', this.pick(['0', '1', '1', '-2', 'x', undefined]))}>${this.opt(`<w:startOverride${this.attr('w:val', this.num())}/>`)}${this.opt(this.lvl('0'), 3)}</w:lvlOverride>`)}</w:num>`);
    return `<?xml version="1.0"?><w:numbering ${W} ${R} ${A} ${V}>${this.opt(body, 12)}</w:numbering>`;
  }

  clr() {
    return this.pick([
      '<a:srgbClr val="1f497d"/>', '<a:srgbClr val=""/>', '<a:srgbClr/>', '<a:srgbClr val="4472c4"><a:lumMod val="75000"/></a:srgbClr>', '<a:sysClr val="window"/>', '<a:sysClr val="windowText"/>',
      '<a:sysClr val="other" lastClr="abcdef"/>', '<a:schemeClr val="accent1"/>', '<a:schemeClr val="phClr"><a:tint val="50000"/></a:schemeClr>', '<a:prstClr val="red"/>', '',
    ]);
  }

  fill() {
    return this.pick([
      () => '<a:noFill/>', () => `<a:solidFill>${this.clr()}</a:solidFill>`,
      () => `<a:gradFill rotWithShape="${this.pick(['1', '0', 'true', ''])}"><a:gsLst>${this.many(4, () => `<a:gs pos="${this.pick(['0', '50000', '100000', 'x', ''])}">${this.clr()}</a:gs>`)}</a:gsLst>${this.opt(`<a:lin ang="${this.num()}" scaled="${this.pick(['1', '0', ''])}"/>`)}${this.opt('<a:path path="circle"><a:fillToRect l="50000" t="50000" r="50000" b="50000"/></a:path>', 4)}</a:gradFill>`,
      () => `<a:pattFill prst="${this.pick(['pct5', 'dkHorz', ''])}"><a:fgClr>${this.clr()}</a:fgClr><a:bgClr>${this.clr()}</a:bgClr></a:pattFill>`,
      () => '<a:blipFill><a:blip r:embed="rId1"/></a:blipFill>', () => '<a:grpFill/>', () => '<a:custom foo="bar"><a:inner/></a:custom>', () => '<a:other>text &amp; more</a:other>',
    ])();
  }

  effect() {
    const shdw = () => `<a:outerShdw blurRad="${this.num()}" dist="${this.num()}" dir="${this.num()}">${this.clr()}</a:outerShdw>`;
    return `<a:effectStyle>${this.many(2, () => `<a:effectLst>${this.many(2, shdw)}</a:effectLst>`)}${this.opt('<a:scene3d/>', 4)}</a:effectStyle>`;
  }

  theme() {
    const font = (tag) => this.opt(`<a:${tag}${this.opt(` typeface="${this.pick(['Calibri', '', 'MS 明朝', 'Times'])}"`)}/>`);
    const fontSet = (tag) => this.opt(`<a:${tag}>${font('latin')}${font('ea')}${font('cs')}${this.opt('<a:font script="Jpan" typeface="游明朝"/>')}${this.opt('<a:font script="Hang"/>')}${this.opt('<a:font typeface="X"/>', 4)}${this.opt('<a:font script="0" typeface="Zero"/>', 4)}${this.opt('<a:font script="__proto__" typeface="P"/>', 6)}${this.opt('<a:font script="Jpan" typeface="Again"/>', 6)}</a:${tag}>`);
    const scheme = this.opt(`<a:clrScheme name="s">${['dk1', 'lt1', 'dk2', 'accent1', 'accent2', 'hlink', 'x:odd', '__proto__', 'constructor', 'dk1'].map((k) => this.opt(`<a:${k}>${this.clr()}</a:${k}>`)).join('')}</a:clrScheme>`);
    const fonts = this.opt(`<a:fontScheme name="f">${fontSet('majorFont')}${fontSet('minorFont')}</a:fontScheme>`);
    const fmt = this.opt(`<a:fmtScheme>${this.opt(`<a:fillStyleLst>${this.many(4, () => this.fill())}</a:fillStyleLst>`)}${this.opt(`<a:lnStyleLst>${this.opt('<a:ln w="6350"/>')}${this.opt('<a:ln/>')}${this.opt('<a:ln w="abc"/>')}${this.opt('<a:ln w="-12700"/>')}${this.opt(`<a:ln w="${this.num()}"/>`)}</a:lnStyleLst>`)}${this.opt(`<a:effectStyleLst>${this.many(4, () => this.effect())}${this.opt('<a:other/>', 4)}</a:effectStyleLst>`)}${this.opt(`<a:bgFillStyleLst>${this.many(3, () => this.fill())}</a:bgFillStyleLst>`)}</a:fmtScheme>`);
    const els = this.pick([scheme + fonts + fmt, fonts + scheme + fmt, fmt + scheme + fonts]);
    return `<a:theme ${A} ${R}><a:themeElements>${els}</a:themeElements></a:theme>`;
  }

  noteProps(tag) {
    return this.opt(`<w:${tag}>${this.opt(`<w:pos${this.attr('w:val', this.pick(['pageBottom', 'sectEnd', '', undefined]))}/>`)}${this.opt(`<w:numFmt${this.attr('w:val', this.pick(['decimal', 'chicago', '', undefined]))}/>`)}${this.opt(`<w:numStart${this.attr('w:val', this.num())}/>`)}${this.opt(`<w:numRestart${this.attr('w:val', this.pick(['continuous', 'eachSect', 'eachPage', 'x', '', undefined]))}/>`)}</w:${tag}>`, 4);
  }

  settings() {
    const flag = (n) => this.opt(`<w:${n}${this.onoff()}/>`, 4);
    const rootFlags = ['doNotUseHTMLParagraphAutoSpacing', 'doNotExpandShiftReturn', 'ulTrailSpace', 'spaceForUL', 'suppressTopSpacing', 'noSpaceRaiseLower'];
    const body = this.opt(`<w:defaultTabStop${this.attr('w:val', this.pick(['708', '720', '12pt', '-5', 'abc', '.', '0', '', '1in', undefined]))}/>`, 3)
      + flag('evenAndOddHeaders') + flag('mirrorMargins') + flag('autoHyphenation') + flag('doNotHyphenateCaps') + flag('displayBackgroundShape') + flag('updateFields')
      + flag('bordersDoNotSurroundHeader') + flag('bordersDoNotSurroundFooter')
      + this.noteProps('footnotePr') + this.noteProps('endnotePr')
      + this.opt(`<w:hyphenationZone${this.attr('w:val', this.pick(['360', '.', '1cm', 'x', '', undefined]))}/>`, 4)
      + this.opt(`<w:consecutiveHyphenLimit${this.attr('w:val', this.num())}/>`, 4)
      + this.opt(`<w:characterSpacingControl${this.attr('w:val', this.pick(['doNotCompress', '', undefined]))}/>`, 4)
      + this.opt(`<w:themeFontLang${this.attr('w:val', this.pick(['pt-BR', '', undefined]))}${this.attr('w:eastAsia', this.pick(['ja-JP', '', undefined]))}${this.attr('w:bidi', this.pick(['ar-SA', '', undefined]))}/>`, 3)
      + this.many(4, () => `<w:${this.pick(rootFlags)}${this.onoff()}/>`)
      + this.opt(`<w:compat>${this.many(6, () => this.pick([
        () => `<w:compatSetting${this.attr('w:name', this.pick(['compatibilityMode', 'other', undefined]))}${this.attr('w:uri', this.pick(['http://schemas.microsoft.com/office/word', 'x', undefined]))}${this.attr('w:val', this.num())}/>`,
        () => `<w:${this.pick(['doNotExpandShiftReturn', 'useFELayout', 'splitPgBreakAndParaMark', '__proto__', 'constructor', 'x5'])}${this.onoff()}/>`,
      ])())}</w:compat>`, 3);
    return `<?xml version="1.0"?><w:settings ${W}>${this.opt(body, 12)}</w:settings>`;
  }

  fontTable() {
    const body = this.many(6, () => {
      const v = (n, pool) => this.opt(`<w:${n}${this.attr('w:val', this.pick(pool))}/>`, 3);
      return `<w:font${this.attr('w:name', this.pick(['Calibri', 'Arial', 'Calibri', '__proto__', '123', '', undefined, 'MS 明朝', 'constructor']))}>${v('altName', ['Alt', '', undefined])}${v('panose1', ['020F0502020204030204', '', undefined])}${v('charset', ['00', '', undefined])}${v('family', ['swiss', 'roman', '', undefined])}${v('pitch', ['variable', '', undefined])}${this.opt(`<w:sig${this.attr('w:usb0', this.pick(['E00002FF', '', undefined]))}${this.attr('w:csb1', 'x')}${this.attr('foo', 'y')}${this.attr('w:5', 'five')}${this.attr('w:__proto__', 'p')}/>`, 3)}</w:font>`;
    });
    return `<?xml version="1.0"?><w:fonts ${W}>${this.opt(body, 12)}</w:fonts>`;
  }

  rels() {
    const types = ['numbering', 'styles', 'theme', 'settings', 'fontTable'];
    const targets = ['numbering.xml', 'styles.xml', 'theme/theme1.xml', 'settings.xml', 'fontTable.xml', './styles.xml', '/word/styles.xml', 'custom/x.xml', '../word/settings.xml', 'missing.xml', '', undefined, 'theme/theme2.xml', 'https://x.test/y.xml'];
    const rel = (i) => `<Relationship${this.attr('Id', `rId${i}`)}${this.attr('Type', `http://schemas.openxmlformats.org/officeDocument/2006/relationships/${this.pick([...types, 'image', 'footer'])}`)}${this.attr('Target', this.pick(targets))}${this.attr('TargetMode', this.pick([undefined, undefined, undefined, 'External', 'Internal']))}/>`;
    let body = '';
    for (let i = 1; i <= this.below(8); i++) body += rel(i);
    return `<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">${body}</Relationships>`;
  }

  numberingRels() {
    const t = (n) => `http://schemas.openxmlformats.org/officeDocument/2006/relationships/${n}`;
    return `<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">`
      + this.many(5, () => `<Relationship${this.attr('Id', this.pick(['rId1', 'rId2', 'rId3', 'rId1', '', undefined]))}${this.attr('Type', t(this.pick(['image', 'image', 'hyperlink'])))}${this.attr('Target', this.pick(['media/a.png', '../media/b.JPEG', '/word/media/a.png', 'media/c.', 'media/d.tar.gz', 'media/e', 'missing.png', 'media/f.p-g', '', undefined]))}${this.attr('TargetMode', this.pick([undefined, undefined, 'External']))}/>`)
      + '</Relationships>';
  }
}

export const QUIRK_SEED = 0x5eed02;
export const QUIRK_COUNT = 300;

const PNG = [0x89, 0x50, 0x4e, 0x47];

/** `count` synthetic inputs: `{ key, parts: [[path, text | bytes]] }`. */
export function quirkParts(seed, count) {
  const out = [];
  for (let i = 0; i < count; i++) {
    const g = new Gen((seed + i * 7919) >>> 0);
    const parts = [];
    const maybe = (path, text, n = 8) => { if (!g.chance(n)) parts.push([path, text()]); };
    const numberingPath = g.chance(5) ? 'word/custom/numbering2.xml' : 'word/numbering.xml';
    maybe('word/styles.xml', () => g.styles());
    maybe(numberingPath, () => g.numbering());
    maybe('word/theme/theme1.xml', () => g.theme());
    maybe('word/settings.xml', () => g.settings());
    maybe('word/fontTable.xml', () => g.fontTable());
    if (g.chance(2)) parts.push(['word/_rels/document.xml.rels', g.chance(3) ? '' : numberingPath === 'word/numbering.xml' ? g.rels() : g.rels().replace('</Relationships>', '<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="custom/numbering2.xml"/></Relationships>')]);
    if (g.chance(2)) {
      const rp = numberingPath.replace(/([^/]*)$/, '_rels/$1.rels');
      parts.push([rp, g.numberingRels()]);
      for (const p of ['word/media/a.png', 'word/media/b.JPEG', 'word/media/d.tar.gz']) if (g.chance(2)) parts.push([p, new Uint8Array([...PNG, g.below(256), g.below(256)])]);
    }
    if (g.chance(12)) parts.push(['word/styles.xml', '']);
    out.push({ key: `quirk/parts/${seed.toString(16)}-${i}.docx`, parts });
  }
  return out;
}

/** The inputs file (one part per line, TAB separated; `\`, LF, CR and TAB escaped; bytes as `hex:`). */
export function inputsText(inputs) {
  const esc = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
  const lines = ['# uo-docx R2 synthetic formatting parts (js/quirk-parts.mjs): `@input <key>`, then `<path>\\t<escaped text | hex:bytes>` per part'];
  for (const { key, parts } of inputs) {
    lines.push(`@input ${key}`);
    for (const [path, data] of parts) lines.push(`${path}\t${typeof data === 'string' ? esc(data) : 'hex:' + Buffer.from(data).toString('hex')}`);
  }
  return lines.join('\n') + '\n';
}
