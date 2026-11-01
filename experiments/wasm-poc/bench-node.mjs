// O motor .NET (UniversalOffice.Docx) em WebAssembly no Node: partida do runtime,
// leitura + layout do .docx (frio e quente), JSON da árvore e PDF.
//   node bench-node.mjs [out/interp|out/aot] [arquivo.docx]
import { readFileSync, writeFileSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { FONT_FILES } from './fonts.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const build = resolve(here, process.argv[2] ?? 'out/interp');
const docxPath = resolve(here, process.argv[3] ?? '../../site/assets/samples/demo.docx');
const fontsDir = resolve(here, '../../site/assets/fonts');

const t0 = performance.now();
const { dotnet } = await import(pathToFileURL(join(build, 'wwwroot/_framework/dotnet.js')).href);
const runtime = await dotnet.create();
const exports = (await runtime.getAssemblyExports('WasmPoc.dll')).WasmPoc.Engine;
const tStart = performance.now() - t0;

let t = performance.now();
for (const f of FONT_FILES) exports.AddFont(f.family, f.bold, f.italic, readFileSync(join(fontsDir, f.path)));
const tFonts = performance.now() - t;

const docx = readFileSync(docxPath);
t = performance.now();
const load = JSON.parse(exports.Load(docx));
const tLoad = performance.now() - t;

const warm = [];
for (let i = 0; i < 5; i++) warm.push(exports.Relayout());

t = performance.now();
const json = exports.LayoutJson();
const tJson = performance.now() - t;

t = performance.now();
const pdf = exports.ToPdf();
const tPdf = performance.now() - t;
writeFileSync(join(here, 'out', 'node-wasm.pdf'), pdf);
writeFileSync(join(here, 'out', 'ref', (process.argv[2] ?? 'out/interp').replace(/[/]/g, '_') + '.layout.json'), json);

const r = (v) => Math.round(v * 10) / 10;
console.log(JSON.stringify({
  build: process.argv[2] ?? 'out/interp',
  startMs: r(tStart), fontsMs: r(tFonts),
  firstLoad: { totalMs: r(tLoad), ...load },
  warmLayoutMs: warm.map(r), warmBestMs: r(Math.min(...warm)),
  layoutJsonMs: r(tJson), layoutJsonKB: Math.round(json.length / 1024),
  pdfMs: r(tPdf), pdfKB: Math.round(pdf.length / 1024),
}, null, 1));
process.exit(0);
