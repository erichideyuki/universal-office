// Writes tests/parity/provider_scenarios.txt: the scenarios of the provider-policy parity facets,
// read by BOTH the captures (capture_viewer.mjs, capture_system.mjs, provider_capture.cs) and
// tests/provider_parity.rs. Deterministic (xorshift32 of uo-testkit). Run again only when a list
// below changes.
//
//   node tests/parity_support/gen_provider_scenarios.mjs
//
// Tab-separated lines. A viewer scenario (`V id` ... `E`):
//   a  family  variant(r|b|i|z)  pool  advances(0|1)     a hosted asset file (Liberation Sans, Aptos, Calibri only)
//   f  family  source  embeddable(0|1)  r=pool,b=pool..  what resolver.getWithFile(family) answers (replaces)
//   c                                                    a new provider() call (a new cache)
//   q  family  bold(0|1)  italic(0|1)                    ask the provider
//   m                                                    dump the missing notes, then loadMissing
// A system scenario (`S id` ... `E`): `s family bold italic` asks, the same provider for the whole scenario.
import { writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Rng } from '../../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const rng = new Rng(0x70726f76);
const pick = (a) => a[rng.below(a.length)];
const chance = (n) => rng.below(n) === 0;

const POOL = ['carlito-regular', 'carlito-bold', 'carlito-italic', 'carlito-bolditalic', 'caladea-regular', 'caladea-bold', 'serif-regular', 'mono-regular'];
const BAD = ['garbage', 'cut'];
const FAMILIES = ['Calibri', 'calibri', 'CALIBRI', 'Liberation Sans', 'liberation sans', 'Aptos', 'Arial', 'arial', 'Times New Roman', 'Carlito', 'Cambria', 'Courier New', 'Own Font', 'Missing Co', 'Caladea', 'Liberation Serif', 'Liberation Mono', 'ΑΣ', 'İstanbul', 'Helvetica'];
const ASSET_FAMILIES = ['Liberation Sans', 'Aptos', 'Calibri'];
const SOURCES = ['host', 'local-access', 'bundled', 'page', 'installed', 'substitute', 'default'];
const V = ['r', 'b', 'i', 'z'];
const SUBS = { calibri: 'Carlito', cambria: 'Caladea', arial: 'Liberation Sans', 'times new roman': 'Liberation Serif', 'courier new': 'Liberation Mono', helvetica: 'Liberation Sans' };

const poolFor = (v) => {
  if (chance(14)) return pick(BAD);
  if (chance(5)) return pick(POOL);
  return { r: 'carlito-regular', b: 'carlito-bold', i: 'carlito-italic', z: 'carlito-bolditalic' }[v];
};
const faceList = () => {
  const vs = V.filter(() => !chance(3));
  if (!vs.length) vs.push(pick(V));
  return vs.map((v) => `${v}=${poolFor(v)}`).join(',');
};
const familyLine = (family) => `f\t${family}\t${pick(SOURCES)}\t${chance(3) ? 0 : 1}\t${faceList()}`;

const lines = [];
for (let id = 0; id < 600; id++) {
  lines.push(`V\t${id}`);
  const assetKeys = new Set();
  if (!chance(3)) {
    for (let k = 0, n = 1 + rng.below(3); k < n; k++) {
      const family = pick(ASSET_FAMILIES), v = pick(V);
      if (assetKeys.has(`${family}|${v}`)) continue;
      assetKeys.add(`${family}|${v}`);
      lines.push(`a\t${family}\t${v}\t${chance(10) ? pick(BAD) : poolFor(v)}\t${chance(2) ? 1 : 0}`);
    }
  }
  const nFam = 1 + rng.below(6);
  for (let k = 0; k < nFam; k++) {
    const family = pick(FAMILIES);
    lines.push(familyLine(family));
    const sub = SUBS[family.toLowerCase()];
    if (sub && chance(2)) lines.push(familyLine(sub));
  }
  const nOps = 4 + rng.below(14);
  for (let k = 0; k < nOps; k++) {
    const r = rng.below(20);
    if (r === 0) lines.push('c');
    else if (r === 1) lines.push('m');
    else if (r === 2) lines.push(familyLine(pick(FAMILIES)));
    else lines.push(`q\t${pick(FAMILIES)}\t${rng.below(2)}\t${rng.below(2)}`);
  }
  lines.push('m');
  lines.push('E');
}

// system scenarios: asks over the families of the generated corpus (tests/provider_support) and of
// shared/fonts/files; the last ones are spellings that only the C# catalog lookup (Trim and
// ToLowerInvariant) tells apart from the TS one
const SYSTEM_FAMILIES = [
  'Test Sans', 'test sans', 'TEST SANS', 'Semi Co', 'Wt Fam', 'Typo', 'Typo Display', 'typo display', 'Typo Shadowed', 'Aptos Like', 'Aptos Like Display',
  'English Name', 'Nom Català', 'Cat Typo', '@Vertical Gothic', 'Vertical Gothic', 'Ünïcode Fönt', 'ünïcode fönt', 'Padded Name', '  Padded Name  ',
  'Slant Co', 'Wordy Co', 'No Sub Co', 'Mac Only', 'Odd Length', 'Broken Face', 'Short Os2', 'Deep Font', 'Backup Only', 'Truncated', 'Coll A', 'Coll B', 'Tie Fam',
  'Carlito', 'carlito', 'Caladea', 'Liberation Serif', 'Liberation Mono', 'Liberation Sans',
  'Calibri', 'calibri', ' CAMBRIA ', 'Cambria', 'Arial', 'arial', 'Arial MT', 'Helvetica', 'Times New Roman', 'Courier New', 'Aptos', 'Nope Fonts', '',
  'CALİBRİ', 'Calibri ', '﻿calibri', 'calibri ', 'Cambria\u0085', 'Ａrial', 'Courier New　', 'Karla', 'TIMES NEW ROMAN',
];
for (let id = 0; id < 40; id++) {
  lines.push(`S\t${id}`);
  const n = 20 + rng.below(30);
  for (let k = 0; k < n; k++) lines.push(`s\t${pick(SYSTEM_FAMILIES)}\t${rng.below(2)}\t${rng.below(2)}`);
  lines.push('E');
}
// every family at the four variants, once, in order
lines.push('S\tall');
for (const f of SYSTEM_FAMILIES) for (const b of [0, 1]) for (const i of [0, 1]) lines.push(`s\t${f}\t${b}\t${i}`);
lines.push('E');

writeFileSync(join(here, '..', 'parity', 'provider_scenarios.txt'), lines.join('\n') + '\n');
console.log(`${lines.length} lines`);
