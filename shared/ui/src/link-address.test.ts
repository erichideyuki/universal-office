import { describe, expect, it } from 'vitest';
import { normalizeLinkAddress, parseLinkSchemes } from './link-address.js';

const WEB = ['http', 'https', 'mailto'] as const;
const address = (input: string, schemes: readonly string[] | null = WEB): string | undefined => {
  const r = normalizeLinkAddress(input, schemes);
  return r.ok ? r.address : undefined;
};
const reason = (input: string, schemes: readonly string[] | null = WEB): string | undefined => {
  const r = normalizeLinkAddress(input, schemes);
  return r.ok ? undefined : r.reason;
};

describe('normalizeLinkAddress com lista de esquemas', () => {
  it('aceita só os esquemas da lista, sem diferenciar maiúsculas', () => {
    expect(address('https://example.com/a?b=c')).toBe('https://example.com/a?b=c');
    expect(address('HTTP://EXAMPLE.COM')).toBe('HTTP://EXAMPLE.COM');
    expect(address('mailto:compliance@example.com')).toBe('mailto:compliance@example.com');
    expect(address('mailto:a@b.com?subject=Ol%C3%A1')).toBe('mailto:a@b.com?subject=Ol%C3%A1');
    expect(reason('ftp://example.com')).toBe('scheme');
    expect(reason('file:///C:/a.docx')).toBe('scheme');
    expect(reason('tel:+5511999999999')).toBe('scheme');
  });

  it('esquemas de script nunca passam, nem se a lista os pedir', () => {
    for (const s of ['javascript:alert(1)', 'JavaScript:alert(1)', 'vbscript:msgbox(1)', 'data:text/html,<b>x</b>', 'blob:https://x/1']) {
      expect(reason(s), s).toBe('scheme');
      expect(reason(s, ['javascript', 'vbscript', 'data', 'blob']), s).toBe('scheme');
      expect(reason(s, null), s).toBe('scheme');
    }
  });

  it('limpa espaços e controles das pontas e TAB/LF/CR do meio, como o navegador', () => {
    expect(address('  https://example.com  ')).toBe('https://example.com');
    expect(address('https://exam\tple.com/a\nb')).toBe('https://example.com/ab');
    expect(reason(' java\tscript:alert(1)')).toBe('scheme');
    expect(address('https://example.com/\u0000')).toBe('https://example.com/');
    expect(reason('https://exa\u0001mple.com')).toBe('invalid');
    expect(reason('https://example.com/\uD800')).toBe('invalid');
  });

  it('sem esquema: e-mail vira mailto e endereço web vira http', () => {
    expect(address('fulano@example.com')).toBe('mailto:fulano@example.com');
    expect(address('www.example.com')).toBe('http://www.example.com');
    expect(address('WWW.Example.com/a')).toBe('http://WWW.Example.com/a');
    expect(address('exemplo.com.br/página?q=1')).toBe('http://exemplo.com.br/página?q=1');
    expect(address('exemplo.com:8080')).toBe('http://exemplo.com:8080');
    expect(address('ação.com.br')).toBe('http://ação.com.br');
  });

  it('nome de arquivo não vira endereço web', () => {
    expect(reason('relatorio.pdf')).toBe('invalid');
    expect(reason('contrato.docx')).toBe('invalid');
  });

  it('só https permitido: o endereço sem esquema ganha https', () => {
    expect(address('www.example.com', ['https'])).toBe('https://www.example.com');
    expect(reason('http://example.com', ['https'])).toBe('scheme');
    expect(reason('fulano@example.com', ['https'])).toBe('scheme');
  });

  it('endereço malformado é recusado', () => {
    expect(reason('')).toBe('empty');
    expect(reason('   ')).toBe('empty');
    for (const s of ['abc invalido', 'foo bar', 'mailto:', 'mailto:fulano', 'http:/x', 'http://', 'https:// example.com', 'http://exa mple.com', 'http:\\\\example.com', '/relativo', '..\\a.docx', 'C:\\docs\\a.docx', '#']) {
      expect(reason(s), s).toBe('invalid');
    }
    expect(reason(`https://example.com/${'a'.repeat(2048)}`)).toBe('invalid');
  });

  it('entrada enorme é recusada sem custo quadrático (espaços colados num href)', () => {
    const huge = `a${' '.repeat(1_000_000)}b`;
    const start = performance.now();
    expect(reason(huge)).toBe('invalid');
    expect(performance.now() - start).toBeLessThan(200);
  });

  it('porta e host como o servidor aceita: porta até 65535, host presente e válido', () => {
    expect(address('https://exemplo.com:8443/a')).toBe('https://exemplo.com:8443/a');
    for (const s of ['https://exemplo.com:99999', 'http://:80', 'http://user@', 'http://%zz', 'exemplo.com:99999']) expect(reason(s), s).toBe('invalid');
  });

  it('domínios da América Latina e outros comuns sem esquema viram http', () => {
    for (const s of ['empresa.com.mx', 'gob.ar', 'empresa.cl', 'empresa.com.pe', 'empresa.ca', 'empresa.nl']) expect(address(s), s).toBe(`http://${s}`);
  });

  it('âncora interna passa com qualquer lista, inclusive vazia', () => {
    expect(address('#_Toc123', [])).toBe('#_Toc123');
    expect(address('#marcador')).toBe('#marcador');
  });

  it('paridade com o servidor do clickCompliance: os aceitos têm esquema da lista e endereço absoluto', () => {
    for (const s of ['https://example.com/a?b=c', 'http://example.com', 'mailto:compliance@example.com']) expect(address(s), s).toBe(s);
    for (const s of ['javascript:alert(1)', 'file:///C:/Windows/win.ini', 'ftp://example.com/file']) expect(reason(s), s).toBe('scheme');
    for (const s of ['abc invalido', 'www.example.com', 'exemplo com espaço']) {
      const r = address(s);
      if (r !== undefined) expect(r, s).toMatch(/^(https?:\/\/[^\s/?#]+|mailto:[^\s@]+@)/i);
    }
  });
});

describe('normalizeLinkAddress sem lista (padrão do componente)', () => {
  it('o endereço fica como veio, menos www e esquemas de script', () => {
    expect(address('outro.docx', null)).toBe('outro.docx');
    expect(address('C:\\docs\\a.docx', null)).toBe('C:\\docs\\a.docx');
    expect(address('file:///C:/a.docx', null)).toBe('file:///C:/a.docx');
    expect(address('ftp://example.com', null)).toBe('ftp://example.com');
    expect(address('www.example.com', null)).toBe('http://www.example.com');
    expect(address('https://example.com', null)).toBe('https://example.com');
    expect(reason('javascript:alert(1)', null)).toBe('scheme');
    expect(reason('', null)).toBe('empty');
  });
});

describe('parseLinkSchemes', () => {
  it('ausente libera tudo, vazio só âncoras, vírgula ou espaço separam', () => {
    expect(parseLinkSchemes(null)).toBeNull();
    expect(parseLinkSchemes('')).toEqual([]);
    expect(parseLinkSchemes('http https,MAILTO')).toEqual(['http', 'https', 'mailto']);
    expect(parseLinkSchemes(' http:, https ')).toEqual(['http', 'https']);
  });
});
