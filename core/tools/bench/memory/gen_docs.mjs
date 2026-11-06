// Generated documents of the memory benchmark (open fonts only: Liberation Serif/Sans, Carlito, Caladea).
// Deterministic (seeded). Classes: typical (~20 pages), tables (~30), images (~20, noise PNGs), pages100, pages500.
//   node core/tools/bench/memory/gen_docs.mjs <outDir>        (UO_TS_ROOT for fflate, as the capture tools)
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { deflateSync, crc32 } from 'node:zlib';

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const NS = `xmlns:r="${R}" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"`;
const WORDS = ('the quick brown fox jumps over the lazy dog document layout engine paragraph table figure memory budget '
  + 'o motor de layout quebra linhas pagina tabelas notas campos desenha cada glifo posicao exata acao coracao informacao '
  + 'hifenizacao configuracao internacionalizacao responsabilidade parágrafo português útil você el motor de diseño').split(' ');
const FAMILIES = ['Liberation Serif', 'Liberation Sans', 'Carlito', 'Caladea'];

function rng(seed) {
  let s = seed >>> 0 || 1;
  return () => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s; };
}
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

function png(w, h, next) {
  // RGB noise rows (filter 0): barely compressible, so the image weighs what a photo does
  const raw = Buffer.alloc((w * 3 + 1) * h);
  for (let y = 0; y < h; y++) {
    raw[y * (w * 3 + 1)] = 0;
    for (let x = 0; x < w * 3; x++) raw[y * (w * 3 + 1) + 1 + x] = (next() & 0xff) ^ ((x * 3 + y) & 0x3f);
  }
  const chunk = (type, data) => {
    const b = Buffer.alloc(12 + data.length);
    b.writeUInt32BE(data.length, 0);
    b.write(type, 4, 'latin1');
    data.copy(b, 8);
    b.writeUInt32BE(crc32(b.subarray(4, 8 + data.length)) >>> 0, 8 + data.length);
    return b;
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8;
  ihdr[9] = 2;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw, { level: 1 })), chunk('IEND', Buffer.alloc(0))]);
}

export function buildDocx({ fflate, seed, paragraphs, tables = 0, tableRows = 0, tableCols = 6, images = 0, imageEvery = 0, imgPx = [320, 240] }) {
  const next = rng(seed);
  const pick = (a) => a[next() % a.length];
  const text = (n) => Array.from({ length: n }, () => pick(WORDS)).join(' ');
  const run = (t, i) => {
    const rpr = [];
    if (i % 7 === 3) rpr.push('<w:b/>');
    if (i % 11 === 5) rpr.push('<w:i/>');
    if (i % 13 === 2) { const f = pick(FAMILIES); rpr.push(`<w:rFonts w:ascii="${f}" w:hAnsi="${f}"/>`); }
    if (i % 17 === 4) rpr.push(`<w:color w:val="${pick(['C00000', '0070C0', '00B050'])}"/>`);
    return `<w:r>${rpr.length ? `<w:rPr>${rpr.join('')}</w:rPr>` : ''}<w:t xml:space="preserve">${esc(t)}</w:t></w:r>`;
  };
  const para = (n) => {
    const ppr = next() % 5 === 0 ? '<w:pPr><w:jc w:val="both"/></w:pPr>' : '';
    const parts = [];
    for (let i = 0; i < 4; i++) parts.push(run(text(Math.max(1, Math.round(n / 4))) + ' ', next() % 20));
    return `<w:p>${ppr}${parts.join('')}</w:p>`;
  };
  const heading = (t) => `<w:p><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="120"/></w:pPr><w:r><w:rPr><w:b/><w:sz w:val="32"/></w:rPr><w:t>${esc(t)}</w:t></w:r></w:p>`;
  const table = (rows, cols) => {
    const w = Math.floor(9000 / cols);
    const b = (s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="808080"/>`;
    let x = `<w:tbl><w:tblPr><w:tblW w:w="${w * cols}" w:type="dxa"/><w:tblBorders>${['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map(b).join('')}</w:tblBorders></w:tblPr><w:tblGrid>${`<w:gridCol w:w="${w}"/>`.repeat(cols)}</w:tblGrid>`;
    for (let r = 0; r < rows; r++) {
      x += `<w:tr>${r === 0 ? '<w:trPr><w:tblHeader/></w:trPr>' : ''}`;
      for (let c = 0; c < cols; c++) {
        const shade = r === 0 ? '<w:shd w:val="clear" w:color="auto" w:fill="D9E2F3"/>' : '';
        x += `<w:tc><w:tcPr><w:tcW w:w="${w}" w:type="dxa"/>${shade}</w:tcPr><w:p>${run(text(2 + (next() % 6)), r === 0 ? 3 : next() % 20)}</w:p></w:tc>`;
      }
      x += '</w:tr>';
    }
    return x + '</w:tbl><w:p/>';
  };
  const media = [];
  const picture = (n) => {
    const [pw, ph] = imgPx;
    media.push(png(pw, ph, next));
    const cx = 3_000_000, cy = Math.round((cx * ph) / pw);
    return `<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="${cx}" cy="${cy}"/><wp:docPr id="${n + 1}" name="Picture ${n}"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="${n + 1}" name="p${n}.png"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rImg${n}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="${cx}" cy="${cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>`;
  };
  let body = '';
  let tablesLeft = tables, imgs = 0;
  const tableEvery = tables ? Math.max(1, Math.floor(paragraphs / tables)) : 0;
  for (let i = 0; i < paragraphs; i++) {
    if (i % 40 === 0) body += heading(`Section ${i / 40 + 1} ${text(3)}`);
    body += para(60 + (next() % 60));
    if (tableEvery && i % tableEvery === tableEvery - 1 && tablesLeft > 0) { body += table(tableRows, tableCols); tablesLeft--; }
    if (imageEvery && i % imageEvery === imageEvery - 1 && imgs < images) body += picture(imgs++);
  }
  const sect = '<w:sectPr><w:headerReference w:type="default" r:id="rH"/><w:footerReference w:type="default" r:id="rF"/><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr>';
  const head = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>';
  const part = (root, inner) => `${head}<w:${root} xmlns:w="${W}" ${NS}>${inner}</w:${root}>`;
  const styles = `${head}<w:styles xmlns:w="${W}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Liberation Serif" w:hAnsi="Liberation Serif"/><w:sz w:val="22"/><w:lang w:val="en-US"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="table" w:default="1" w:styleId="TN"><w:name w:val="Normal Table"/><w:tblPr><w:tblCellMar><w:left w:w="108" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style></w:styles>`;
  const pageField = '<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>';
  const over = (p, t) => `<Override PartName="/${p}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.${t}+xml"/>`;
  const ct = `${head}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/>${over('word/document.xml', 'document.main')}${over('word/styles.xml', 'styles')}${over('word/header1.xml', 'header')}${over('word/footer1.xml', 'footer')}</Types>`;
  let rels = `${head}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rS" Type="${R}/styles" Target="styles.xml"/><Relationship Id="rH" Type="${R}/header" Target="header1.xml"/><Relationship Id="rF" Type="${R}/footer" Target="footer1.xml"/>`;
  const files = {
    '[Content_Types].xml': fflate.strToU8(ct),
    '_rels/.rels': fflate.strToU8(`${head}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${R}/officeDocument" Target="word/document.xml"/></Relationships>`),
    'word/document.xml': fflate.strToU8(`${head}<w:document xmlns:w="${W}" ${NS}><w:body>${body}${sect}</w:body></w:document>`),
    'word/styles.xml': fflate.strToU8(styles),
    'word/header1.xml': fflate.strToU8(part('hdr', '<w:p><w:r><w:rPr><w:i/></w:rPr><w:t>Memory benchmark document</w:t></w:r></w:p>')),
    'word/footer1.xml': fflate.strToU8(part('ftr', pageField)),
  };
  media.forEach((m, i) => {
    files[`word/media/image${i}.png`] = new Uint8Array(m);
    rels += `<Relationship Id="rImg${i}" Type="${R}/image" Target="media/image${i}.png"/>`;
  });
  files['word/_rels/document.xml.rels'] = fflate.strToU8(rels + '</Relationships>');
  return fflate.zipSync(files, { level: 6 });
}

// paragraphs per class (calibrated against the Rust layout; run.mjs reports the page counts)
export const CLASSES = {
  typical: { seed: 11, paragraphs: 125 },
  tables: { seed: 12, paragraphs: 40, tables: 18, tableRows: 12, tableCols: 6 },
  images: { seed: 13, paragraphs: 90, images: 40, imageEvery: 2, imgPx: [320, 240] },
  pages100: { seed: 14, paragraphs: 620 },
  pages500: { seed: 15, paragraphs: 3130 },
};

export async function loadFflate(root) {
  return import(pathToFileURL(join(root, 'write/node_modules/fflate/esm/index.mjs')).href);
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const out = resolve(process.argv[2] ?? '.');
  const root = process.env.UO_TS_ROOT;
  if (!root) throw new Error('set UO_TS_ROOT (fflate is taken from its write/node_modules)');
  const fflate = await loadFflate(root);
  mkdirSync(out, { recursive: true });
  for (const [name, spec] of Object.entries(CLASSES)) {
    const bytes = buildDocx({ fflate, ...spec });
    writeFileSync(join(out, `${name}.docx`), bytes);
    console.log(`${name}.docx ${bytes.length} bytes`);
  }
}
