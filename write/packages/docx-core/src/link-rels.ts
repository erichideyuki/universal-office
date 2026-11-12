/**
 * Relações de hiperlink que o próprio editor criou (`rIdWnLink<n>`, externas)
 * saem do .rels quando nada na parte as cita mais. O pacote é o mesmo entre
 * gravações; sem a poda, um link removido continuava no .rels (e um endereço
 * que o hospedeiro recusa travava as gravações seguintes). Relações do arquivo
 * original (do Word, de imagem…) nunca são tocadas.
 */
import type { DocxPackage } from './package.js';

const EDITOR_LINK_REL = /<Relationship\b[^>]*\bId="(rIdWnLink\d+)"[^>]*\/>/g;
const HYPERLINK_TYPE = /\bType="[^"]*\/relationships\/hyperlink"/;
const EXTERNAL = /\bTargetMode="External"/;
const RELS_PATH = /^(word\/(?:[^/]+\/)*)_rels\/([^/]+)\.rels$/;

/** O .rels sem as relações `rIdWnLink<n>` de hiperlink externo que `partXml` não cita em nenhum atributo. */
export function dropUnusedEditorLinkRels(relsXml: string, partXml: string): string {
  let out = relsXml;
  for (const [tag, id] of relsXml.matchAll(EDITOR_LINK_REL)) {
    if (!HYPERLINK_TYPE.test(tag) || !EXTERNAL.test(tag) || partXml.includes(`="${id}"`)) continue;
    out = out.replace(tag, '');
  }
  return out;
}

/** Aplica `dropUnusedEditorLinkRels` ao .rels de cada parte de `word/`, pelo XML da própria parte. */
export function pruneEditorLinkRels(pkg: DocxPackage): void {
  for (const relsPath of pkg.partNames()) {
    const m = RELS_PATH.exec(relsPath);
    if (!m) continue;
    const partXml = pkg.getPartText(`${m[1]}${m[2]}`);
    const relsXml = pkg.getPartText(relsPath);
    if (partXml === undefined || partXml === null || !relsXml) continue;
    const next = dropUnusedEditorLinkRels(relsXml, partXml);
    if (next !== relsXml) pkg.setPartText(relsPath, next);
  }
}
