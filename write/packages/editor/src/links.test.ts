// Endereço de hiperlink no <universal-write> de verdade (DOM falso): a lista
// `link-schemes` do hospedeiro vale na API, na colagem e no diálogo; sem ela, o
// comportamento é o de sempre (link para arquivo), só com `www.` virando
// `http://`; e a relação de link que o editor criou some do .rels quando o link
// sai do documento, mesmo gravando de novo o mesmo pacote.
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { blankDocxPackage, DocxPackage, type Paragraph, type RunProps } from '@universal-office/docx-core';
import { installFakeDom, type FakeDom, type FakeNode } from '../../../test-support/fake-dom.js';
import { MiniDOMParser } from '../../../test-support/mini-html.js';
import type { UniversalWriteElement as Editor } from './universal-write.js';
import type { UniversalWriteFontAsset } from './plugins.js';

const FONTS = join(__dirname, '..', 'demo', 'fonts');
const VARIANTS = { regular: 'regular', bold: 'bold', italic: 'italic', boldItalic: 'bolditalic' } as const;
const FONT_ASSETS: UniversalWriteFontAsset[] = Object.entries(VARIANTS).map(([variant, file]) => ({
  family: 'Liberation Sans', variant: variant as UniversalWriteFontAsset['variant'],
  data: new Uint8Array(readFileSync(join(FONTS, `liberationsans-${file}.ttf`))),
}));

const W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"';
const HYPERLINK = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink';
const p = (inner: string): string => `<w:p>${inner}</w:p>`;
const r = (t: string): string => `<w:r><w:t xml:space="preserve">${t}</w:t></w:r>`;

function docx(body: string, rels: string[] = []): Uint8Array {
  const pkg = blankDocxPackage();
  pkg.setPartText('word/document.xml', `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document ${W}><w:body>${body}`
    + '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>');
  if (rels.length) {
    const path = 'word/_rels/document.xml.rels';
    const xml = pkg.getPartText(path) ?? '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>';
    pkg.setPartText(path, xml.replace('</Relationships>', `${rels.join('')}</Relationships>`));
  }
  return pkg.save();
}

const PLAIN = docx(p(r('Olá mundo')) + p(r('Segunda linha')));
/** documento do Word com um link próprio (rId7) que continua referenciado */
const WITH_WORD_LINK = docx(
  p(`<w:hyperlink r:id="rId7">${r('portal')}</w:hyperlink>`) + p(r('Olá mundo')),
  [`<Relationship Id="rId7" Type="${HYPERLINK}" Target="https://portal.example.com" TargetMode="External"/>`],
);

let dom: FakeDom;
let Ctor: typeof Editor;
let ed: Editor;

type Internals = { model: { paragraphs: Paragraph[] }; undoStack: unknown[] };
const I = (): Internals => ed as unknown as Internals;
const paras = (): Paragraph[] => I().model.paragraphs;
const runOf = (t: string): RunProps => paras().flatMap((q) => q.runs).find((x) => x.text.includes(t))!.props;
const select = (para: number, from: number, to: number): void => ed.setSelection({ para, offset: from }, { para, offset: to });
const rels = (bytes: Uint8Array): string => DocxPackage.load(bytes).getPartText('word/_rels/document.xml.rels') ?? '';

beforeAll(async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] });
  dom = installFakeDom();
  vi.stubGlobal('DOMParser', MiniDOMParser);
  Ctor = (await import('./universal-write.js')).UniversalWriteElement;
}, 60_000);
afterAll(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

beforeEach(async () => {
  ed = new Ctor();
  ed.setAttribute('manual-init', '');
  dom.document.body.appendChild(ed as unknown as FakeNode);
  await ed.initialize({ fonts: FONT_ASSETS });
  ed.load(PLAIN);
});
afterEach(() => {
  (ed as unknown as FakeNode).remove();
  vi.clearAllTimers();
});

describe('linkSchemes: atributo e propriedade', () => {
  it('ausente = sem restrição; atributo com vírgula ou espaço; a propriedade manda depois de definida', () => {
    expect(ed.linkSchemes).toBeNull();
    ed.setAttribute('link-schemes', 'http https,mailto');
    expect(ed.linkSchemes).toEqual(['http', 'https', 'mailto']);
    ed.linkSchemes = ['https'];
    expect(ed.linkSchemes).toEqual(['https']);
  });
});

describe('insertHyperlink e setLink com link-schemes', () => {
  beforeEach(() => ed.setAttribute('link-schemes', 'http https mailto'));

  it('endereço inválido devolve false e não muda o documento nem o desfazer', () => {
    select(0, 0, 3);
    const undo = I().undoStack.length;
    expect(ed.insertHyperlink({ address: 'abc invalido' })).toBe(false);
    expect(ed.insertHyperlink({ address: 'file:///C:/a.docx' })).toBe(false);
    expect(ed.insertHyperlink({ address: 'javascript:alert(1)' })).toBe(false);
    expect(runOf('Olá').link).toBeUndefined();
    expect(I().undoStack.length).toBe(undo);
  });

  it('www vira http e e-mail vira mailto; o texto fica como foi digitado', () => {
    select(0, 0, 3);
    expect(ed.insertHyperlink({ address: 'www.example.com' })).toBe(true);
    expect(runOf('Olá').link).toBe('http://www.example.com');
    select(1, 0, 7);
    expect(ed.insertHyperlink({ address: 'fulano@example.com' })).toBe(true);
    expect(runOf('Segunda').link).toBe('mailto:fulano@example.com');
  });

  it('âncora interna continua valendo', () => {
    select(0, 0, 3);
    expect(ed.insertHyperlink({ address: '#marcador' })).toBe(true);
    expect(runOf('Olá').link).toBe('#marcador');
  });

  it('setLink devolve false para endereço recusado e normaliza o aceito', () => {
    select(0, 0, 3);
    expect(ed.setLink('abc invalido')).toBe(false);
    expect(runOf('Olá').link).toBeUndefined();
    expect(ed.setLink('www.example.com')).toBe(true);
    expect(runOf('Olá').link).toBe('http://www.example.com');
  });
});

describe('sem link-schemes (padrão do componente)', () => {
  it('link para arquivo continua aceito; esquema de script não', () => {
    select(0, 0, 3);
    expect(ed.insertHyperlink({ address: 'file:///C:/docs/a.docx' })).toBe(true);
    expect(runOf('Olá').link).toBe('file:///C:/docs/a.docx');
    select(1, 0, 7);
    expect(ed.insertHyperlink({ address: 'javascript:alert(1)' })).toBe(false);
    expect(runOf('Segunda').link).toBeUndefined();
  });
});

describe('colar com link-schemes', () => {
  beforeEach(() => {
    ed.setAttribute('link-schemes', 'http https mailto');
    select(1, 0, 0);
  });

  it('link colado com endereço recusado entra como texto sem link; sem esquema é normalizado', () => {
    const clip = {
      text: 'ruimbomscript',
      paras: [{ runs: [
        { text: 'ruim', props: { link: 'abc invalido', linkTip: 'dica' } },
        { text: 'bom', props: { link: 'www.example.com' } },
        { text: 'script', props: { link: 'javascript:alert(1)' } },
      ] }],
    };
    ed.pasteData({ text: clip.text, rich: clip as never });
    expect(runOf('ruim').link).toBeUndefined();
    expect(runOf('ruim').linkTip).toBeUndefined();
    expect(runOf('bom').link).toBe('http://www.example.com');
    expect(runOf('script').link).toBeUndefined();
  });

  it('link recusado perde também o azul e o sublinhado que vinham com ele', () => {
    const clip = { text: 'azul', paras: [{ runs: [{ text: 'azul', props: { link: 'abc invalido', color: '0000FF', underline: true, bold: true } }] }] };
    ed.pasteData({ text: clip.text, rich: clip as never });
    expect(runOf('azul').link).toBeUndefined();
    expect(runOf('azul').color).toBeUndefined();
    expect(runOf('azul').underline).toBeFalsy();
    expect(runOf('azul').bold).toBe(true);
  });

  it('HTML colado: href fora da lista vira texto sem link', () => {
    ed.pasteData({ text: '', html: '<p><a href="abc invalido">ruim</a> e <a href="file:///C:/a.docx">arquivo</a> e <a href="https://example.com">bom</a></p>' });
    expect(runOf('ruim').link).toBeUndefined();
    expect(runOf('arquivo').link).toBeUndefined();
    expect(runOf('bom').link).toBe('https://example.com');
  });
});

describe('gravação: relação de link criada pelo editor', () => {
  it('remover o link e gravar de novo no mesmo pacote apaga a relação; a do Word continua', () => {
    ed.load(WITH_WORD_LINK);
    select(1, 0, 3);
    expect(ed.insertHyperlink({ address: 'https://example.com/novo' })).toBe(true);
    const first = rels(ed.saveDocx());
    expect(first).toMatch(/Id="rIdWnLink\d+"[^>]*Target="https:\/\/example\.com\/novo"/);
    expect(first).toContain('Id="rId7"');
    select(1, 0, 3);
    expect(ed.removeLink()).toBe(true);
    const second = rels(ed.saveDocx());
    expect(second).not.toContain('https://example.com/novo');
    expect(second).not.toContain('rIdWnLink');
    expect(second).toContain('Id="rId7"');
  });
});

describe('diálogo Inserir Hiperlink: validação do endereço', () => {
  it('endereço inválido ou fora da lista dá a mensagem; válido e vazio passam', async () => {
    const { hyperlinkAddressError } = await import('./chrome-refs.js');
    ed.setAttribute('link-schemes', 'http https mailto');
    expect(hyperlinkAddressError(ed, 'abc invalido', 'pt')).toBe('Digite um endereço válido, como https://www.exemplo.com.br.');
    expect(hyperlinkAddressError(ed, 'file:///C:/a.docx', 'pt')).toBe('Este tipo de endereço não é permitido aqui.');
    expect(hyperlinkAddressError(ed, 'abc invalido', 'en')).toBe('Enter a valid address, such as https://www.example.com.');
    expect(hyperlinkAddressError(ed, 'www.example.com', 'pt')).toBeNull();
    expect(hyperlinkAddressError(ed, 'mailto:fulano@example.com?subject=Ol%C3%A1', 'pt')).toBeNull();
    expect(hyperlinkAddressError(ed, '', 'pt')).toBeNull();
  });

  it('na aba de e-mail, o erro pede um e-mail válido', async () => {
    const { hyperlinkAddressError } = await import('./chrome-refs.js');
    ed.setAttribute('link-schemes', 'http https mailto');
    expect(hyperlinkAddressError(ed, 'mailto:abc', 'pt', 'mail')).toBe('Digite um e-mail válido, como nome@exemplo.com.br.');
  });
});
