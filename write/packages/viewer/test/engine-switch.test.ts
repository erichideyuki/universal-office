/**
 * A troca TS/Rust do <universal-document-viewer> (atributo `engine`): sem `engine="rust"` o viewer NUNCA toca o .wasm (nem o
 * chunk do motor); com `engine="rust"` (só com UO_WASM_PATH, o build `--features write` de core/crates/uo-wasm) abre os mesmos
 * documentos que o motor TS e confere páginas, tamanhos, títulos, comentários, caixas de link e indicadores, e que remover o
 * elemento devolve todos os handles do módulo. A comparação de pixels e da camada de texto roda no Chromium
 * (core/crates/uo-wasm/tests/browser/viewer_switch.mjs). Elemento de verdade sobre o DOM falso (write/test-support); fontes
 * ABERTAS versionadas servidas por um fetch falso.
 */
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { blankDocxPackage } from '@universal-office/docx-core';
import { installFakeDom, type FakeDom, type FakeNode } from '../../../test-support/fake-dom.js';
import type { UniversalDocumentViewerElement as Viewer } from '../src/universal-document-viewer.js';

const ROOT = join(__dirname, '..', '..', '..', '..');
const FONT_DIRS = [join(ROOT, 'write', 'packages', 'editor', 'demo', 'fonts'), join(ROOT, 'shared', 'fonts', 'files')];
const FONTS_BASE = 'http://fonts.test';
const WASM = process.env['UO_WASM_PATH'];
const FIXTURES = join(ROOT, 'core', 'crates', 'uo-docx-layout', 'tests', 'captures', 'f8', 'docs');

const requested: string[] = [];
/** fetch falso: as fontes abertas do repositório em http://fonts.test/<arquivo>; o resto, 404. Anota cada URL. */
const fakeFetch = async (input: string | URL): Promise<Response> => {
  const url = String(input);
  requested.push(url);
  if (url.startsWith(`${FONTS_BASE}/`)) {
    const file = url.slice(FONTS_BASE.length + 1);
    for (const dir of FONT_DIRS) {
      const path = join(dir, file);
      if (/^[\w.-]+\.(ttf|json)$/.test(file) && existsSync(path)) return new Response(new Uint8Array(readFileSync(path)));
    }
  }
  return new Response(null, { status: 404 });
};

const W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"';
const r = (t: string): string => `<w:r><w:t xml:space="preserve">${t}</w:t></w:r>`;

/** .docx: texto, link externo, link interno para um indicador e uma 2ª página. */
function sampleDocx(): Uint8Array {
  const pkg = blankDocxPackage();
  pkg.setPartText('word/_rels/document.xml.rels', '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    + '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    + '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>'
    + '<Relationship Id="rIdWeb" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://exemplo.test/politica" TargetMode="External"/>'
    + '</Relationships>');
  pkg.setPartText('word/document.xml', `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document ${W}><w:body>`
    + `<w:p>${r('Veja ')}<w:hyperlink r:id="rIdWeb">${r('o site')}</w:hyperlink>${r(' e leia mais.')}</w:p>`
    + `<w:p><w:hyperlink w:anchor="cap2">${r('capítulo 2')}</w:hyperlink></w:p>`
    + '<w:p><w:r><w:br w:type="page"/></w:r></w:p>'
    + `<w:p><w:bookmarkStart w:id="0" w:name="cap2"/>${r('Capítulo 2')}<w:bookmarkEnd w:id="0"/></w:p>`
    + '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr>'
    + '</w:body></w:document>');
  return pkg.save();
}

type Internals = {
  linkBoxes: { page: number; x: number; y: number; w: number; h: number; value: string }[][];
  layout: { engine?: string; pages: { widthTw: number; heightTw: number }[] } | null;
  rust: { hasDocument: boolean } | null;
};

let dom: FakeDom;
let Ctor: typeof Viewer;
const live: Viewer[] = [];

beforeAll(async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] });
  dom = installFakeDom();
  vi.stubGlobal('fetch', fakeFetch);
  Ctor = (await import('../src/universal-document-viewer.js')).UniversalDocumentViewerElement;
}, 60_000);
afterAll(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
afterEach(() => {
  for (const el of live.splice(0)) (el as unknown as FakeNode).remove();
  vi.clearAllTimers();
});

const connected = (engine?: string): Viewer => {
  const el = new Ctor();
  el.setAttribute('fonts', FONTS_BASE);
  if (engine !== undefined) el.setAttribute('engine', engine);
  dom.document.body.appendChild(el as unknown as FakeNode);
  live.push(el);
  return el;
};

describe('sem engine="rust" o viewer não toca o .wasm', () => {
  for (const engine of [undefined, 'ts', 'bogus', '']) {
    it(`engine=${JSON.stringify(engine)}: carrega pelo motor TS, sem fetch de .wasm nem WebAssembly.instantiate`, async () => {
      const instantiate = vi.spyOn(WebAssembly, 'instantiate');
      const compile = vi.spyOn(WebAssembly, 'compile');
      requested.length = 0;
      const v = connected(engine);
      await v.loadDocx(sampleDocx());
      expect(v.getPageCount()).toBeGreaterThan(1);
      expect((v as unknown as Internals).layout?.engine).toBeUndefined();
      expect((v as unknown as Internals).rust).toBeNull();
      expect(requested.filter((u) => /\.wasm($|\?)/.test(u) || /rust-engine/.test(u))).toEqual([]);
      expect(instantiate).not.toHaveBeenCalled();
      expect(compile).not.toHaveBeenCalled();
      instantiate.mockRestore();
      compile.mockRestore();
    });
  }
});

describe.skipIf(!WASM)('engine="rust" contra o motor TS (UO_WASM_PATH)', () => {
  const docs: [string, () => Uint8Array][] = [
    ['amostra', sampleDocx],
    ...['f8__7__0', 'f8__7__1', 'f8__7__3'].map((k): [string, () => Uint8Array] => [k, () => new Uint8Array(readFileSync(join(FIXTURES, `${k}.docx`)))]),
  ];

  it('mesmas páginas, tamanhos, títulos, comentários, caixas de link e indicador; remover devolve os handles', async () => {
    Ctor.configureEngine({ wasm: new Uint8Array(readFileSync(WASM!)) });
    for (const [name, load] of docs) {
      const ts = connected();
      const rust = connected('rust');
      await ts.loadDocx(load());
      await rust.loadDocx(load());
      const a = ts as unknown as Internals, b = rust as unknown as Internals;
      expect(b.layout?.engine, name).toBe('rust');
      expect(rust.getPageCount(), name).toBe(ts.getPageCount());
      expect(b.layout!.pages.map((p) => [p.widthTw, p.heightTw]), name).toEqual(a.layout!.pages.map((p) => [p.widthTw, p.heightTw]));
      expect(rust.getHeadings(), name).toEqual(ts.getHeadings());
      expect(rust.getComments(), name).toEqual(ts.getComments());
      const boxes = (x: Internals): unknown[][] => x.linkBoxes.map((page) => page.map(({ page: pg, x: bx, y, w, h, value }) => ({ pg, bx, y, w, h, value })));
      expect(boxes(b), name).toEqual(boxes(a));
      expect(rust.getLinks().length, name).toBe(ts.getLinks().length);
      expect(rust.goToBookmark('cap2'), name).toBe(ts.goToBookmark('cap2'));
      expect(rust.goToBookmark('nao-existe'), name).toBe(false);
      expect(rust.exportPdf().length, name).toBeGreaterThan(1000);
      console.log(`engine-switch ${name}: ${rust.getPageCount()} pages, ${rust.getHeadings().length} headings, ${rust.getComments().length} comments, ${b.linkBoxes.flat().length} link boxes, ${rust.getLinks().length} links`);
    }
    const engine = await import('@universal-office/write-engine');
    expect(engine.viewerModuleLoaded()).toBe(true);
    for (const el of live.splice(0)) (el as unknown as FakeNode).remove();
    expect((await engine.loadViewerModule()).stats().handles).toBe(0);
  }, 120_000);
});
