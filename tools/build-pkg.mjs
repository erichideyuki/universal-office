#!/usr/bin/env node
/**
 * build-pkg — build de PUBLICAÇÃO: cada pacote publicado sai pronto em
 * `<pacote>/publish/` (fora do git). Publicar = `npm publish` DENTRO dessa
 * pasta. O `dist/` continua sendo a saída do tsc que os pacotes do monorepo
 * consomem entre si. Três tipos de pacote:
 *
 * PRODUTOS (os web components: write, document-viewer, compare, sheet,
 * slides-viewer, slides) — o código vem AUTOSSUFICIENTE:
 *   - index.js: ESM minificado com TODO o código dentro — os pacotes internos
 *     (@universal-office/* pelo src) e as dependências npm (Material, fflate…);
 *     produto com mais de uma entrada (`entries`) ganha um arquivo por entrada
 *     na raiz (o write: index.js = <universal-write>, viewer.js = só o
 *     <universal-document-viewer>), com os chunks compartilhados — e o build
 *     confere que a entrada extra não carrega, estaticamente, o código da principal;
 *   - chunks/: o que o código já importa sob demanda (padrões de hifenização,
 *     barra do editor, conversões raras) — baixado só quando usado;
 *   - chunks/uo-write.wasm e chunks/uo-write-editor.wasm: o motor Rust (`engine="rust"`), só o que o
 *     código carrega. Dois módulos, escolhidos pelo CONTEÚDO do bundle (não pelo nome): o do viewer
 *     (`--features write`, sem `uo_session_*`) vai nos pacotes cujo bundle importa o viewer-rust-engine
 *     (document-viewer; o write, quando só o viewer o usa); o do editor (`--features session`) vai no
 *     pacote cujo bundle importa o rust-engine do editor (write). REGRA DE ORDEM: um módulo de motor por
 *     página. O módulo do editor serve também o <universal-document-viewer> do MESMO pacote carregado
 *     DEPOIS dele (a memória do motor é uma só); um motor de viewer carregado ANTES faz o `engine="rust"`
 *     do editor falhar com erro claro. O pacote document-viewer avulso leva a própria cópia do viewer,
 *     logo o próprio módulo (e a própria memória). `shipEngineWasm` recusa build de teste (exports
 *     test/selftest/memtest/bench/fixture, alocador contador), editor sem `uo_session_open` e viewer COM ele;
 *     UO_WRITE_WASM / UO_WRITE_EDITOR_WASM dizem de onde vêm (padrão core/target/pkg-wasm/);
 *     UO_REQUIRE_ENGINE_WASM=1 torna a falta do arquivo fatal (CI); UO_TOOLS_ROOT (só desenvolvimento)
 *     pega esbuild, dts-bundle-generator e node_modules de OUTRO checkout (worktree sem node_modules);
 *   - index.d.ts (e um .d.ts por entrada extra): tipos num arquivo só (os dos
 *     pacotes internos embutidos);
 *   - licenses/<pacote>/: os textos de licença de terceiros que os pacotes
 *     internos EMBUTIDOS no bundle declaram em `licenseFiles` no package.json;
 *   - package.json publicável + README.md + LICENSE (MIT). As únicas
 *     `dependencies` são os pacotes de DADOS deste repositório listados em
 *     `dependencies` abaixo (as fontes), sempre com a versão EXATA (sem ^).
 *
 * FONTES (`@universal-office/font-files`, shared/fonts/packages/font-files):
 * só as fontes ABERTAS (Liberation, Carlito, Caladea) e os textos de licença
 * delas, na raiz do pacote, + README. Saíram dos produtos (eram as mesmas 7 MB
 * em cada um): o host serve a pasta do pacote e aponta para ela o atributo
 * `fonts` / FontResolver.shared().configure({ baseUrl }).
 *
 * DADOS da revisão ortográfica (shared/spell/packages/*): o motor
 * (`@universal-office/spellcheck` = src/hunspell/wasm.ts do spell) e um pacote
 * por dicionário (`@universal-office/dictionary-<id>` = src/dicts/<id>.ts) —
 * index.js (um módulo ES só, sem chunks), index.d.ts, licenses/ com os
 * `licenseFiles` do package.json do pacote, README. São OPCIONAIS: o write não
 * depende deles; o host os registra (registerSpellcheck/registerDictionary) ou
 * aponta a pasta (atributo `spellcheck-url`). O build confere que nenhum
 * produto embute o motor ou um dicionário.
 *
 * BIBLIOTECA (`@universal-office/converter`, converter/packages/converter): um
 * index.js só, universal (browser e Node), com os leitores OOXML internos
 * (docs/sheets/slides-processing) dentro; as dependências npm (fflate, pdfjs-dist)
 * ficam de fora, em `dependencies`, com a faixa do package.json.
 *
 * Os pacotes internos (fonts, charts, drawingml, ui, spell, docx-core, docx-layout,
 * sheet-core, sheet-calc, slides-core) são `private` e NUNCA são publicados: só existem
 * dentro dos bundles. O bloco `publish` do package.json de cada pacote dá a
 * descrição/palavras-chave/licença do manifesto publicado; página, repositório
 * (com a pasta do pacote), issues e patrocínio são os mesmos para todos
 * (`PROJECT`, abaixo).
 *
 * Uso: node tools/build-pkg.mjs [pacote…]   (sem argumentos: todos)
 */
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { verifyNotices } from '../core/tools/licenses/gen-notices.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const p = (...s) => path.join(ROOT, ...s);

/** Tipos dos pacotes de dados da revisão (o módulo só exporta constantes). */
const SPELLCHECK_DTS = `/**
 * Hunspell 1.7.2 compiled to WebAssembly, for the spell checker of <universal-write>.
 * Register it with registerSpellcheck(() => import('@universal-office/spellcheck')),
 * or serve this folder and use the editor's \`spellcheck-url\` attribute.
 */
/** Source of the emscripten ES module (default export = module factory). */
export declare const glue: string;
/** The WebAssembly binary, base64. */
export declare const wasmBase64: string;
`;
const dictionaryDts = (id) => `/**
 * The ${id} Hunspell dictionary for the spell checker of <universal-write>.
 * Register it with registerDictionary('${id}', () => import('@universal-office/dictionary-${id.toLowerCase()}')),
 * or serve this folder and use the editor's \`spellcheck-url\` attribute.
 */
/** Contents of the .aff file (UTF-8). */
export declare const aff: string;
/** Contents of the .dic file (UTF-8). */
export declare const dic: string;
`;

/**
 * Pacotes publicados: id → pasta (package.json + README.md; a saída vai para
 * `<pasta>/publish/`). Produtos: `project` = workspace dono (de onde vêm esbuild
 * e node_modules); `entries` = entradas extras (nome do arquivo → src);
 * `dependencies` = pacotes de dados daqui que o manifesto exige (versão exata).
 * Dados: `entry` = o módulo publicado; `types` = o index.d.ts.
 */
export const PRODUCTS = {
  write: {
    dir: 'write/packages/editor', project: 'write',
    entries: { viewer: 'write/packages/viewer/src/index.ts' },
    dependencies: ['font-files'],
  },
  'document-viewer': { dir: 'write/packages/viewer', project: 'write', dependencies: ['font-files'] },
  compare: { dir: 'write/packages/compare', project: 'write' },
  sheet: { dir: 'sheet/packages/sheet', project: 'sheet', dependencies: ['font-files'] },
  'slides-viewer': { dir: 'slides/packages/viewer', project: 'slides', dependencies: ['font-files'] },
  slides: { dir: 'slides/packages/slides', project: 'slides', dependencies: ['font-files'] },
  'font-files': { dir: 'shared/fonts/packages/font-files', kind: 'fonts' },
  spellcheck: { dir: 'shared/spell/packages/spellcheck', project: 'write', kind: 'data', entry: 'shared/spell/src/hunspell/wasm.ts', types: SPELLCHECK_DTS },
  'dictionary-pt-br': { dir: 'shared/spell/packages/dictionary-pt-br', project: 'write', kind: 'data', entry: 'shared/spell/src/dicts/pt-BR.ts', types: dictionaryDts('pt-BR') },
  'dictionary-en-us': { dir: 'shared/spell/packages/dictionary-en-us', project: 'write', kind: 'data', entry: 'shared/spell/src/dicts/en-US.ts', types: dictionaryDts('en-US') },
  'dictionary-es': { dir: 'shared/spell/packages/dictionary-es', project: 'write', kind: 'data', entry: 'shared/spell/src/dicts/es.ts', types: dictionaryDts('es') },
  converter: { dir: 'converter/packages/converter', project: 'converter', kind: 'library' },
};

/**
 * O lado JS do motor Rust do viewer (`engine="rust"`): o alias que o viewer importa. FORA de INTERNAL de propósito: o laço
 * das licenças lê `<pacote>/package.json` de cada INTERNAL e `core/crates/uo-wasm` não é um pacote; e os tipos do viewer
 * declaram a superfície por `write/packages/viewer/src/write-engine.d.ts` (módulo ambiente), sem `paths`.
 */
const ENGINE = {
  '@universal-office/write-engine': 'core/crates/uo-wasm/js/viewer.ts',
  // o editor (`engine="rust"`, src/rust-engine.ts) importa a sessão de edição: sem este alias o `build:pkg write` não resolve
  '@universal-office/write-session': 'core/crates/uo-wasm/js/session.mjs',
  // o viewer do Slides (`engine="rust"`, src/rust-engine.ts, frente B7): o lado JS do módulo uo-slides.wasm
  '@universal-office/slides-engine': 'core/crates/uo-wasm/js/slides-engine.ts',
};
/**
 * Os .wasm do motor (build `--profile wasm`, SEM `count` nem `selftest`): `--features write` → `chunks/uo-write.wasm` (viewer) e
 * `--features session` → `chunks/uo-write-editor.wasm` (editor). O caminho padrão NÃO é a saída do cargo (que muitas vezes
 * guarda um build de teste: S11); `checkEngineWasm` confere o conteúdo de qualquer jeito.
 */
const WRITE_WASM = () => process.env.UO_WRITE_WASM ?? 'core/target/pkg-wasm/uo-write.wasm';
const EDITOR_WASM = () => process.env.UO_WRITE_EDITOR_WASM ?? 'core/target/pkg-wasm/uo-write-editor.wasm';
/** Os fontes do lado JS do motor, por tipo de módulo (o que o bundle importa decide qual .wasm vai junto). */
const ENGINE_SOURCES = { viewer: 'write/packages/viewer/src/rust-engine.ts', editor: 'write/packages/editor/src/rust-engine.ts', slides: 'slides/packages/viewer/src/rust-engine.ts' };

/**
 * Ferramentas de build (esbuild, dts-bundle-generator, node_modules). Normalmente `<projeto>/node_modules` deste checkout;
 * UO_TOOLS_ROOT (só desenvolvimento, inerte sem ela) aponta outro checkout que as tenha (worktree sem node_modules).
 */
const TOOLS_ROOT = process.env.UO_TOOLS_ROOT ? path.resolve(process.env.UO_TOOLS_ROOT) : null;
const TOOL_PROJECTS = ['write', 'converter', 'sheet', 'slides', 'shared/fonts', 'shared/ui', 'shared/spell', 'shared/charts', 'shared/drawingml'];
const toolsRequire = (project) => createRequire(path.join(TOOLS_ROOT ?? ROOT, project, 'package.json'));
const toolsNodePaths = (project) => (TOOLS_ROOT
  ? TOOL_PROJECTS.map((d) => path.join(TOOLS_ROOT, d, 'node_modules')).filter((d) => existsSync(d))
  : [p(project, 'node_modules')]);

/** Pacotes internos → src (embutidos em todo bundle que os usa). */
const INTERNAL = {
  '@universal-office/fonts': 'shared/fonts/src/index.ts',
  '@universal-office/charts': 'shared/charts/src/index.ts',
  '@universal-office/drawingml': 'shared/drawingml/src/index.ts',
  '@universal-office/ui': 'shared/ui/src/index.ts',
  '@universal-office/spell': 'shared/spell/src/index.ts',
  // só o registro de onde vêm motor e dicionários (o write o exporta sem levar o SpellChecker)
  '@universal-office/spell-sources': 'shared/spell/src/sources.ts',
  '@universal-office/docx-core': 'write/packages/docx-core/src/index.ts',
  '@universal-office/docx-layout': 'write/packages/docx-layout/src/index.ts',
  '@universal-office/compare': 'write/packages/compare/src/index.ts',
  '@universal-office/sheet-core': 'sheet/packages/sheet-core/src/index.ts',
  '@universal-office/sheet-calc': 'sheet/packages/calc/src/index.ts',
  // só a colação do Windows do sheet-calc (Classificar do Write), sem o motor de cálculo
  '@universal-office/collation': 'sheet/packages/calc/src/collation.ts',
  '@universal-office/slides-core': 'slides/packages/slides-core/src/index.ts',
  '@universal-office/slides-viewer': 'slides/packages/viewer/src/index.ts',
  // o conversor (publicado à parte, como biblioteca) e os leitores OOXML dele
  '@universal-office/converter': 'converter/packages/converter/src/index.ts',
  '@universal-office/docs-processing': 'converter/packages/docs-processing/src/index.ts',
  '@universal-office/sheets-processing': 'converter/packages/sheets-processing/src/index.ts',
  '@universal-office/slides-processing': 'converter/packages/slides-processing/src/index.ts',
};

/** Fontes ABERTAS (OFL/Apache) que vão para o @universal-office/font-files. */
const OPEN_FONT_DIRS = ['shared/fonts/files', 'write/packages/editor/demo/fonts'];
const OPEN_FONT = (f) => /^(liberation(sans|serif|mono)|carlito|caladea)-[a-z]+\.ttf$/.test(f) || /^LICENSE-(liberation|carlito|caladea)\.txt$/.test(f);

/** Dados da revisão: nunca dentro de um produto (são os pacotes spellcheck/dictionary-*). */
const SPELL_DATA = /[\\/]shared[\\/]spell[\\/]src[\\/](dicts|hunspell)[\\/]/;

const readJson = (f) => JSON.parse(readFileSync(f, 'utf8'));

/** package.json do pacote + pasta publish/ limpa. */
function prepare(id) {
  const pkgDir = p(PRODUCTS[id].dir);
  const src = readJson(path.join(pkgDir, 'package.json'));
  const dist = path.join(pkgDir, 'publish');
  rmSync(dist, { recursive: true, force: true }); // publish/ só tem arquivos gerados (sem junções)
  mkdirSync(dist, { recursive: true });
  return { pkgDir, src, publish: src.publish ?? {}, dist };
}

/** dependencies do manifesto: os pacotes daqui, com a versão EXATA do repositório. */
function dependenciesOf(id) {
  const deps = PRODUCTS[id].dependencies ?? [];
  if (!deps.length) return undefined;
  return Object.fromEntries(deps.map((dep) => {
    const pkg = readJson(p(PRODUCTS[dep].dir, 'package.json'));
    return [pkg.name, pkg.version];
  }));
}

/** Metadados do projeto, iguais em todos os pacotes publicados. */
const PROJECT = {
  homepage: 'https://universaloffice.org/',
  repository: 'https://github.com/erichideyuki/universal-office',
  sponsor: 'https://github.com/sponsors/erichideyuki',
};

/** Manifesto: os campos do bloco `publish` + o que o tipo do pacote pede. */
function writeManifest(dist, src, publish, fields) {
  const manifest = {
    name: src.name,
    version: src.version,
    description: publish.description ?? src.description,
    homepage: PROJECT.homepage,
    // `directory`: o npmjs.com liga à pasta do pacote no repositório
    repository: { type: 'git', url: `git+${PROJECT.repository}.git`, directory: path.relative(ROOT, path.dirname(dist)).split(path.sep).join('/') },
    bugs: { url: `${PROJECT.repository}/issues` },
    funding: { type: 'github', url: PROJECT.sponsor },
    ...fields,
    ...(publish.keywords ? { keywords: publish.keywords } : {}),
    ...(publish.license ? { license: publish.license } : {}),
  };
  if (!manifest.dependencies) delete manifest.dependencies;
  writeFileSync(path.join(dist, 'package.json'), JSON.stringify(manifest, null, 2) + '\n');
}

/**
 * README: os GIFs moram uma vez só em site/assets/gifs (o link relativo vale no
 * repositório); no pacote eles vão para media/ e os links entre READMEs viram texto.
 * O npmjs.com só resolve link relativo com `repository` no manifesto: o GIF aponta
 * para a cópia do próprio pacote publicado no jsDelivr.
 */
function writeReadme(pkgDir, dist, src) {
  const readme = path.join(pkgDir, 'README.md');
  if (!existsSync(readme)) return false;
  let md = readFileSync(readme, 'utf8');
  let media = false;
  md = md.replace(/\]\((?:\.\.\/)+site\/assets\/gifs\/([\w.-]+\.gif)\)/g, (_, gif) => {
    mkdirSync(path.join(dist, 'media'), { recursive: true });
    copyFileSync(p('site/assets/gifs', gif), path.join(dist, 'media', gif));
    media = true;
    return `](https://cdn.jsdelivr.net/npm/${src.name}@${src.version}/media/${gif})`;
  });
  md = md.replace(/\[([^\]]+)\]\((?:\.\.\/)+[^)]*README\.md[^)]*\)/g, '$1');
  writeFileSync(path.join(dist, 'README.md'), md);
  return media;
}

/** Copia os `licenseFiles` (relativos à pasta do pacote) para `to/`. */
function copyLicenseFiles(fromDir, files, to) {
  for (const lf of files) {
    mkdirSync(to, { recursive: true });
    copyFileSync(path.join(fromDir, lf), path.join(to, path.basename(lf)));
  }
  return files.length > 0;
}

/** Saídas que um arquivo do bundle carrega ESTATICAMENTE (ele incluso), pelo metafile. */
function staticClosure(metafile, out) {
  const seen = new Set();
  const visit = (o) => {
    if (seen.has(o) || !metafile.outputs[o]) return;
    seen.add(o);
    for (const imp of metafile.outputs[o].imports) if (imp.kind === 'import-statement') visit(imp.path);
  };
  visit(out);
  return [...seen];
}

/**
 * Tipos num arquivo só por entrada: tsconfig temporário com os internos pelo src. O dts-bundle-generator monta o programa só
 * com os ARQUIVOS DE ENTRADA, então os módulos ambientes do pacote (`declare module '@universal-office/write-engine'`,
 * `'@universal-office/write-session'`: `src/*.d.ts`) não entram e o build falha (TS2307). Cada entrada vira um arquivo
 * de apoio com `/// <reference path>` para esses .d.ts + `export *` da entrada real (as entradas não têm export default).
 */
function writeTypes(pkgDir, dist, entryPoints, entries = {}) {
  const tsconfig = path.join(pkgDir, '.tsconfig.pkg.json');
  const rel = (f) => path.relative(pkgDir, p(f)).replace(/\\/g, '/');
  const fwd = (f) => f.replace(/\\/g, '/');
  const srcDirs = [path.join(pkgDir, 'src'), ...Object.values(entries).map((f) => path.dirname(p(f)))];
  const ambient = srcDirs.flatMap((d) => (existsSync(d) ? readdirSync(d).filter((f) => f.endsWith('.d.ts')).sort().map((f) => path.join(d, f)) : []));
  const compilerOptions = { noEmit: false, rootDir: path.relative(pkgDir, ROOT).replace(/\\/g, '/') || '.', baseUrl: '.', paths: Object.fromEntries(Object.entries(INTERNAL).map(([n, f]) => [n, [rel(f)]])) };
  if (TOOLS_ROOT) { // checkout sem node_modules: tipos e dependências vêm de UO_TOOLS_ROOT
    compilerOptions.typeRoots = [fwd(path.join(TOOLS_ROOT, 'write', 'node_modules', '@types'))];
    compilerOptions.paths['*'] = toolsNodePaths('write').map((d) => fwd(path.join(d, '*')));
  }
  writeFileSync(tsconfig, JSON.stringify({
    extends: './tsconfig.json',
    // rootDir na raiz: os fontes dos pacotes internos entram no programa
    compilerOptions,
    include: ['src', ...Object.values(entries).map((f) => rel(path.dirname(f)))],
  }, null, 2));
  const wrappers = [];
  try {
    const dts = toolsRequire('write').resolve('dts-bundle-generator/dist/bin/dts-bundle-generator.js');
    for (const [name, entry] of Object.entries(entryPoints)) {
      const wrapper = path.join(pkgDir, `.pkg-entry-${name}.ts`);
      wrappers.push(wrapper);
      const refs = ambient.map((a) => `/// <reference path="${fwd(a)}" />`).join('\n');
      writeFileSync(wrapper, `${refs}\nexport * from '${fwd(entry).replace(/\.ts$/, '.js')}';\n`);
      execFileSync(process.execPath, [dts, '--project', tsconfig, '--no-check', '--silent', '-o', path.join(dist, `${name}.d.ts`), wrapper], { cwd: pkgDir, stdio: 'inherit' });
    }
  } finally {
    rmSync(tsconfig, { force: true });
    for (const w of wrappers) rmSync(w, { force: true });
  }
}

/** Onde ficam os avisos de licença do núcleo Rust (gerados por core/tools/licenses/gen-notices.mjs, versionados). */
const NOTICES = process.env.UO_RUST_NOTICES ?? 'core/licenses';

/** Exports de um build de teste (test_/selftest_/memtest_/bench_/fixture_ no meio do nome, ou _fixture no fim). */
const TEST_EXPORT = /(^|_)(test|selftest|memtest|bench|fixture)_|_fixture$/;

/**
 * Confere o CONTEÚDO de um .wasm do motor (S11: o caminho padrão do cargo já guardou um build de teste), sem cargo: compila
 * com `WebAssembly.Module`, recusa exports de teste, instancia com imports que lançam e recusa o alocador contador
 * (`uo_memory_live_bytes() !== -1`). `kind`: 'viewer' (SEM `uo_session_open`) ou 'editor' (COM ele: build `session`).
 */
export function checkEngineWasm(file, kind) {
  let module;
  try { module = new WebAssembly.Module(readFileSync(file)); } catch (e) { throw new Error(`${file} não compila como WebAssembly (${e.message})`); }
  const exports = WebAssembly.Module.exports(module).map((e) => e.name);
  const tests = exports.filter((n) => TEST_EXPORT.test(n));
  if (tests.length) throw new Error(`${file} é um build de teste (exports: ${tests.slice(0, 6).join(', ')}${tests.length > 6 ? `, … +${tests.length - 6}` : ''}); use cargo build -p uo-wasm --features ${kind === 'editor' ? 'session' : 'write'} --target wasm32-unknown-unknown --profile wasm, sem count nem selftest`);
  const imports = {};
  for (const i of WebAssembly.Module.imports(module)) {
    if (i.kind !== 'function') throw new Error(`${file} importa ${i.module}.${i.name} (${i.kind}), que não é função`);
    (imports[i.module] ??= {})[i.name] = () => { throw new Error(`import ${i.module}.${i.name} chamado na verificação`); };
  }
  const live = new WebAssembly.Instance(module, imports).exports.uo_memory_live_bytes;
  if (typeof live !== 'function' || live() !== -1) throw new Error(`${file} tem o alocador contador (build com a feature count)`);
  const session = exports.includes('uo_session_open');
  if (kind === 'editor' && !session) throw new Error(`${file} não tem uo_session_open: o editor precisa do build --features session`);
  if (kind === 'viewer' && session) throw new Error(`${file} é o build session (tem uo_session_open): o uo-write.wasm do viewer é o build --features write`);
}

/**
 * Copia o .wasm do motor Rust para `dist/chunks/uo-write.wasm` (kind 'viewer') ou `uo-write-editor.wasm` (kind 'editor') e, junto,
 * os avisos de licença das caixas Rust de terceiros compiladas nele (`dist/licenses/uo-write-wasm/`, a união dos dois).
 * Antes de copiar confere o conteúdo (checkEngineWasm). Sem o .wasm só avisa (SW-9) e devolve false (com UO_REQUIRE_ENGINE_WASM=1
 * lança); com o .wasm e sem avisos atuais (faltam ou o Cargo.lock mudou) o build FALHA. Devolve true quando copiou os avisos.
 */
export function shipEngineWasm(id, dist, kind = 'viewer') {
  const editor = kind === 'editor';
  const name = editor ? 'uo-write-editor.wasm' : 'uo-write.wasm';
  const env = editor ? 'UO_WRITE_EDITOR_WASM' : 'UO_WRITE_WASM';
  const source = editor ? EDITOR_WASM() : WRITE_WASM();
  const wasm = path.isAbsolute(source) ? source : p(source);
  if (!existsSync(wasm)) {
    const msg = `${path.relative(ROOT, wasm)} não existe (cargo build -p uo-wasm --features ${editor ? 'session' : 'write'} --target wasm32-unknown-unknown --profile wasm e copiar para lá, ou ${env})`;
    if (process.env.UO_REQUIRE_ENGINE_WASM === '1') throw new Error(`build-pkg: ${msg}; ${id} não pode sair sem ${name}`);
    console.warn(`build-pkg: AVISO: ${msg}; engine="rust" não funciona em ${id}`);
    return false;
  }
  try { checkEngineWasm(wasm, kind); } catch (e) { throw new Error(`build-pkg: ${id}/chunks/${name}: ${e.message}`); }
  mkdirSync(path.join(dist, 'chunks'), { recursive: true });
  copyFileSync(wasm, path.join(dist, 'chunks', name));
  console.log(`build-pkg: ${id}/chunks/${name} (${(statSync(wasm).size / 1024).toFixed(0)} KB)`);
  const notices = path.isAbsolute(NOTICES) ? NOTICES : p(NOTICES);
  try {
    const files = verifyNotices(notices, p('core/Cargo.lock'), editor ? 'wasm-editor' : 'wasm');
    for (const f of files) {
      const to = path.join(dist, 'licenses', 'uo-write-wasm', f);
      mkdirSync(path.dirname(to), { recursive: true });
      copyFileSync(path.join(notices, f), to);
    }
  } catch (e) {
    throw new Error(`build-pkg: ${id} leva chunks/${name} sem os avisos de licença das caixas Rust (${e.message})`);
  }
  return true;
}

/**
 * Os .wasm do Slides (build `--profile wasm`, SEM `count`): `--features slides` → `chunks/uo-slides.wasm` (o viewer, `engine="rust"`) e
 * `--features slides-ppt` → `chunks/uo-slides-ppt.wasm` (o conversor de .ppt, carregado só ao abrir um .ppt). Por padrão em
 * `core/target/pkg-wasm/` (UO_SLIDES_WASM, UO_SLIDES_PPT_WASM); com os avisos de licença das caixas Rust de terceiros de cada um
 * (`licenses/uo-slides-wasm/`). Sem o .wasm só avisa (como o Write); com ele e sem avisos atuais o build FALHA.
 */
export function shipSlidesWasm(id, dist) {
  const MODULES = [
    { name: 'uo-slides.wasm', env: 'UO_SLIDES_WASM', feature: 'slides', artifact: 'wasm-slides', needs: 'uo_slides_open', never: 'uo_slides_ppt_convert' },
    { name: 'uo-slides-ppt.wasm', env: 'UO_SLIDES_PPT_WASM', feature: 'slides-ppt', artifact: 'wasm-slides-ppt', needs: 'uo_slides_ppt_convert', never: 'uo_slides_open' },
  ];
  let shipped = false;
  for (const m of MODULES) {
    const source = process.env[m.env] ?? `core/target/pkg-wasm/${m.name}`;
    const wasm = path.isAbsolute(source) ? source : p(source);
    if (!existsSync(wasm)) {
      const msg = `${path.relative(ROOT, wasm)} não existe (cargo build -p uo-wasm --features ${m.feature} --target wasm32-unknown-unknown --profile wasm e copiar para lá, ou ${m.env})`;
      if (process.env.UO_REQUIRE_ENGINE_WASM === '1') throw new Error(`build-pkg: ${msg}; ${id} não pode sair sem ${m.name}`);
      console.warn(`build-pkg: AVISO: ${msg}; engine="rust" não funciona em ${id}`);
      continue;
    }
    let module;
    try { module = new WebAssembly.Module(readFileSync(wasm)); } catch (e) { throw new Error(`build-pkg: ${id}/chunks/${m.name}: ${wasm} não compila como WebAssembly (${e.message})`); }
    const exports = WebAssembly.Module.exports(module).map((e) => e.name);
    const tests = exports.filter((n) => TEST_EXPORT.test(n));
    if (tests.length) throw new Error(`build-pkg: ${id}/chunks/${m.name}: ${wasm} é um build de teste (exports: ${tests.slice(0, 6).join(', ')}); use --features ${m.feature} sem count`);
    if (!exports.includes(m.needs) || exports.includes(m.never)) throw new Error(`build-pkg: ${id}/chunks/${m.name}: ${wasm} não é o build --features ${m.feature} (espera ${m.needs}, sem ${m.never})`);
    const imports = {};
    for (const i of WebAssembly.Module.imports(module)) {
      if (i.kind !== 'function') throw new Error(`build-pkg: ${id}/chunks/${m.name}: importa ${i.module}.${i.name} (${i.kind}), que não é função`);
      (imports[i.module] ??= {})[i.name] = () => { throw new Error(`import ${i.module}.${i.name} chamado na verificação`); };
    }
    const live = new WebAssembly.Instance(module, imports).exports.uo_memory_live_bytes;
    if (typeof live !== 'function' || live() !== -1) throw new Error(`build-pkg: ${id}/chunks/${m.name}: ${wasm} tem o alocador contador (build com a feature count)`);
    mkdirSync(path.join(dist, 'chunks'), { recursive: true });
    copyFileSync(wasm, path.join(dist, 'chunks', m.name));
    console.log(`build-pkg: ${id}/chunks/${m.name} (${(statSync(wasm).size / 1024).toFixed(0)} KB)`);
    const notices = path.isAbsolute(NOTICES) ? NOTICES : p(NOTICES);
    try {
      for (const f of verifyNotices(notices, p('core/Cargo.lock'), m.artifact)) {
        const to = path.join(dist, 'licenses', 'uo-slides-wasm', f);
        mkdirSync(path.dirname(to), { recursive: true });
        copyFileSync(path.join(notices, f), to);
      }
    } catch (e) {
      throw new Error(`build-pkg: ${id} leva chunks/${m.name} sem os avisos de licença das caixas Rust (${e.message})`);
    }
    shipped = true;
  }
  return shipped;
}

async function buildProduct(id) {
  const { project, entries = {} } = PRODUCTS[id];
  const { pkgDir, src, publish, dist } = prepare(id);
  const esbuild = toolsRequire(project)('esbuild');

  // 1. JS: tudo dentro, partes sob demanda (e as compartilhadas entre entradas) em chunks/
  const alias = Object.fromEntries([...Object.entries(INTERNAL).filter(([n]) => n !== src.name), ...Object.entries(ENGINE)].map(([n, f]) => [n, p(f)]));
  const entryPoints = { index: path.join(pkgDir, 'src', 'index.ts'), ...Object.fromEntries(Object.entries(entries).map(([n, f]) => [n, p(f)])) };
  const built = await esbuild.build({
    metafile: true,
    entryPoints,
    outdir: dist, bundle: true, format: 'esm', platform: 'browser', target: 'es2022',
    minify: true, splitting: true, chunkNames: 'chunks/[name]-[hash]', legalComments: 'linked',
    alias, absWorkingDir: pkgDir, nodePaths: toolsNodePaths(project), logLevel: 'warning',
  });
  const inputs = Object.keys(built.metafile.inputs).map((f) => path.resolve(pkgDir, f));
  const spellData = inputs.filter((f) => SPELL_DATA.test(f));
  if (spellData.length) throw new Error(`build-pkg: ${id} embute dados da revisão (devem ficar nos pacotes spellcheck/dictionary-*): ${spellData.map((f) => path.relative(ROOT, f)).join(', ')}`);

  // 1b. Entrada extra: o que ela carrega ao importar não pode trazer o código da
  //     principal (o viewer.js do write não leva o editor)
  for (const name of Object.keys(entries)) {
    const out = path.relative(pkgDir, path.join(dist, `${name}.js`)).replace(/\\/g, '/');
    const closure = staticClosure(built.metafile, out);
    const own = closure.flatMap((o) => Object.keys(built.metafile.outputs[o].inputs))
      .map((f) => path.resolve(pkgDir, f)).filter((f) => f.startsWith(path.join(pkgDir, 'src') + path.sep));
    if (own.length) throw new Error(`build-pkg: ${id}/${name}.js carrega código da entrada principal: ${own.map((f) => path.relative(ROOT, f)).join(', ')}`);
    const kb = (o) => (built.metafile.outputs[o].bytes / 1024).toFixed(0);
    console.log(`build-pkg: ${id}/${name}.js importa estaticamente ${closure.length - 1} chunk(s): ${closure.map((o) => `${path.posix.relative(path.relative(pkgDir, dist).replace(/\\/g, '/'), o)} (${kb(o)} KB)`).join(', ')}`);
  }

  // 1c. O(s) .wasm do motor Rust (engine="rust") ao lado do chunk que carrega o lado JS, com os avisos de licença. Quem decide é o
  //     CONTEÚDO do bundle (metafile), não o nome do chunk: o rust-engine do viewer e o do editor ficam em chunks diferentes.
  let withLicenses = false;
  const outDir = path.relative(pkgDir, dist).replace(/\\/g, '/');
  for (const kind of ['viewer', 'editor', 'slides']) {
    const holders = Object.entries(built.metafile.outputs).filter(([, o]) => Object.keys(o.inputs).some((f) => path.resolve(pkgDir, f) === p(ENGINE_SOURCES[kind])));
    if (!holders.length) continue;
    for (const [o] of holders) {
      if (path.posix.dirname(o) !== `${outDir}/chunks`) throw new Error(`build-pkg: ${id}: o motor ${kind} (${o}) não está em chunks/: new URL(arquivo, import.meta.url) não acharia o .wasm ao lado`);
    }
    if (kind === 'slides' ? shipSlidesWasm(id, dist) : shipEngineWasm(id, dist, kind)) withLicenses = true;
  }
  // fase 0: o import estático da entrada principal não traz o lado JS do motor (nem o wasm: só com engine="rust")
  const phase0 = staticClosure(built.metafile, `${outDir}/index.js`).flatMap((o) => Object.keys(built.metafile.outputs[o].inputs))
    .map((f) => path.resolve(pkgDir, f)).filter((f) => Object.values(ENGINE_SOURCES).some((e) => f === p(e)) || f.startsWith(p('core/crates/uo-wasm/js') + path.sep));
  if (phase0.length) throw new Error(`build-pkg: ${id}/index.js carrega o motor Rust estaticamente (fase 0): ${phase0.map((f) => path.relative(ROOT, f)).join(', ')}`);

  // 2. Tipos num arquivo só por entrada
  writeTypes(pkgDir, dist, entryPoints, entries);

  // 3. Licenças de terceiros dos pacotes internos que entraram no bundle: cada
  //    um declara os textos em `licenseFiles` (package.json) → licenses/<nome>/
  for (const [name, entry] of Object.entries(INTERNAL)) {
    const dir = path.dirname(path.dirname(p(entry))); // <pacote>/src/index.ts → <pacote>
    if (!inputs.some((f) => f.startsWith(dir + path.sep))) continue;
    const files = readJson(path.join(dir, 'package.json')).licenseFiles ?? [];
    if (copyLicenseFiles(dir, files, path.join(dist, 'licenses', name.replace(/^@universal-office\//, '')))) withLicenses = true;
  }

  // 4. Manifesto publicável: as fontes (font-files) como dependência de versão fixa
  const names = Object.keys(entryPoints);
  const sub = (n) => (n === 'index' ? '.' : `./${n}`);
  const media = writeReadme(pkgDir, dist, src);
  writeManifest(dist, src, publish, {
    type: 'module',
    main: './index.js',
    module: './index.js',
    types: './index.d.ts',
    exports: Object.fromEntries(names.map((n) => [sub(n), { types: `./${n}.d.ts`, import: `./${n}.js` }])),
    // o componente se auto-registra (customElements.define) ao importar
    sideEffects: true,
    files: [...names.flatMap((n) => [`${n}.js`, `${n}.d.ts`]), 'chunks', '*.LEGAL.txt', ...(withLicenses ? ['licenses'] : []), ...(media ? ['media'] : []), 'README.md', 'LICENSE'],
    dependencies: dependenciesOf(id),
  });
  // a licença do projeto (MIT) vai junto: o texto precisa acompanhar as cópias
  copyFileSync(p('LICENSE'), path.join(dist, 'LICENSE'));
  console.log(`build-pkg: ${src.name}@${src.version} → ${path.relative(ROOT, dist)}`);
}

/** @universal-office/font-files: as fontes abertas e as licenças delas, na raiz. */
function buildFonts(id) {
  const { pkgDir, src, publish, dist } = prepare(id);
  for (const dir of OPEN_FONT_DIRS) {
    for (const f of readdirSync(p(dir))) {
      if (OPEN_FONT(f) && !existsSync(path.join(dist, f))) copyFileSync(p(dir, f), path.join(dist, f));
    }
  }
  writeReadme(pkgDir, dist, src);
  writeManifest(dist, src, publish, { files: ['*.ttf', 'LICENSE-*.txt', 'README.md'] });
  console.log(`build-pkg: ${src.name}@${src.version} → ${path.relative(ROOT, dist)}`);
}

/** Motor ou dicionário da revisão: um módulo ES só (sem chunks) + licenças. */
async function buildData(id) {
  const { project, entry, types } = PRODUCTS[id];
  const { pkgDir, src, publish, dist } = prepare(id);
  const esbuild = toolsRequire(project)('esbuild');
  await esbuild.build({
    entryPoints: { index: p(entry) },
    outdir: dist, bundle: true, format: 'esm', platform: 'neutral', target: 'es2022',
    // UTF-8 cru (script de módulo é sempre lido como UTF-8): o "á" do dicionário
    // ocupa 2 bytes em vez dos 4 do escape á
    minify: true, charset: 'utf8', legalComments: 'linked', logLevel: 'warning',
  });
  writeFileSync(path.join(dist, 'index.d.ts'), types);
  const withLicenses = copyLicenseFiles(pkgDir, src.licenseFiles ?? [], path.join(dist, 'licenses'));
  writeReadme(pkgDir, dist, src);
  writeManifest(dist, src, publish, {
    type: 'module',
    main: './index.js',
    module: './index.js',
    types: './index.d.ts',
    exports: { '.': { types: './index.d.ts', import: './index.js' } },
    sideEffects: false,
    files: ['index.js', 'index.d.ts', '*.LEGAL.txt', ...(withLicenses ? ['licenses'] : []), 'README.md'],
  });
  console.log(`build-pkg: ${src.name}@${src.version} → ${path.relative(ROOT, dist)}`);
}

/**
 * Biblioteca (@universal-office/converter): um módulo ES só, universal (browser e
 * Node), com os pacotes internos dentro; as dependências npm ficam de fora e vão
 * para `dependencies` do manifesto, com a faixa de versão do package.json.
 */
async function buildLibrary(id) {
  const { project } = PRODUCTS[id];
  const { pkgDir, src, publish, dist } = prepare(id);
  const esbuild = toolsRequire(project)('esbuild');
  const npmDeps = Object.fromEntries(Object.entries(src.dependencies ?? {}).filter(([n]) => !INTERNAL[n]));
  const alias = Object.fromEntries(Object.entries(INTERNAL).filter(([n]) => n !== src.name).map(([n, f]) => [n, p(f)]));
  const entryPoints = { index: path.join(pkgDir, 'src', 'index.ts') };
  await esbuild.build({
    entryPoints,
    outdir: dist, bundle: true, format: 'esm', platform: 'neutral', target: 'es2022',
    external: Object.keys(npmDeps).flatMap((n) => [n, `${n}/*`]),
    minify: true, legalComments: 'linked', alias, absWorkingDir: pkgDir, logLevel: 'warning',
  });
  writeTypes(pkgDir, dist, entryPoints);
  writeReadme(pkgDir, dist, src);
  writeManifest(dist, src, publish, {
    type: 'module',
    main: './index.js',
    module: './index.js',
    types: './index.d.ts',
    exports: { '.': { types: './index.d.ts', import: './index.js' } },
    sideEffects: false,
    files: ['index.js', 'index.d.ts', '*.LEGAL.txt', 'README.md', 'LICENSE'],
    ...(src.engines ? { engines: src.engines } : {}),
    dependencies: Object.keys(npmDeps).length ? npmDeps : undefined,
  });
  copyFileSync(p('LICENSE'), path.join(dist, 'LICENSE'));
  console.log(`build-pkg: ${src.name}@${src.version} → ${path.relative(ROOT, dist)}`);
}

async function build(id) {
  const kind = PRODUCTS[id].kind;
  if (kind === 'fonts') return buildFonts(id);
  if (kind === 'data') return buildData(id);
  if (kind === 'library') return buildLibrary(id);
  return buildProduct(id);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const ids = process.argv.slice(2);
  for (const id of ids.length ? ids : Object.keys(PRODUCTS)) {
    if (!PRODUCTS[id]) { console.error(`build-pkg: pacote desconhecido "${id}" (${Object.keys(PRODUCTS).join(', ')})`); process.exit(1); }
    await build(id);
  }
}
