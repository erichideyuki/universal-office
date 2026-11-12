import { describe, expect, it } from 'vitest';
import { blankDocxPackage, dropUnusedEditorLinkRels, pruneEditorLinkRels } from '../src/index.js';

const HL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink';
const IMG = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/image';
const rel = (id: string, type: string, target: string, external = true): string =>
  `<Relationship Id="${id}" Type="${type}" Target="${target}"${external ? ' TargetMode="External"' : ''}/>`;
const rels = (...items: string[]): string =>
  `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">${items.join('')}</Relationships>`;

describe('dropUnusedEditorLinkRels', () => {
  it('tira só a relação rIdWnLink externa de hyperlink que a parte não cita mais', () => {
    const xml = rels(
      rel('rIdWnLink1', HL, 'abc invalido'),
      rel('rIdWnLink2', HL, 'https://example.com/citado'),
      rel('rIdWnLink10', HL, 'https://example.com/dez'),
      rel('rId7', HL, 'https://word.example.com'),
      rel('rIdWnLink3', IMG, 'media/image1.png', false),
    );
    const part = '<w:p><w:hyperlink r:id="rIdWnLink2"/><w:hyperlink r:id="rIdWnLink10"/></w:p>';
    const out = dropUnusedEditorLinkRels(xml, part);
    expect(out).not.toContain('rIdWnLink1"');
    expect(out).toContain('Id="rIdWnLink2"');
    expect(out).toContain('Id="rIdWnLink10"');
    expect(out).toContain('Id="rId7"');
    expect(out).toContain('Id="rIdWnLink3"');
  });

  it('referência em qualquer atributo (desenho, outro prefixo) mantém a relação', () => {
    const xml = rels(rel('rIdWnLink1', HL, 'https://example.com'));
    expect(dropUnusedEditorLinkRels(xml, '<a:hlinkClick xmlns:a="x" rel:id="rIdWnLink1"/>')).toBe(xml);
  });

  it('sem nada a remover devolve a mesma string', () => {
    const xml = rels(rel('rId7', HL, 'https://word.example.com'));
    expect(dropUnusedEditorLinkRels(xml, '<w:p/>')).toBe(xml);
  });
});

describe('pruneEditorLinkRels', () => {
  it('poda o .rels de cada parte pelo XML da própria parte', () => {
    const pkg = blankDocxPackage();
    pkg.setPartText('word/document.xml', '<w:document><w:body><w:p/></w:body></w:document>');
    pkg.setPartText('word/_rels/document.xml.rels', rels(rel('rIdWnLink1', HL, 'https://example.com/fora')));
    pkg.setPartText('word/header1.xml', '<w:hdr><w:hyperlink r:id="rIdWnLink1"/></w:hdr>');
    pkg.setPartText('word/_rels/header1.xml.rels', rels(rel('rIdWnLink1', HL, 'https://example.com/cabecalho')));
    pruneEditorLinkRels(pkg);
    expect(pkg.getPartText('word/_rels/document.xml.rels')).not.toContain('rIdWnLink1');
    expect(pkg.getPartText('word/_rels/header1.xml.rels')).toContain('rIdWnLink1');
  });
});
