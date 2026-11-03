// Captures the `oracle` provider policy: `createNodeFontProvider` of write/tools/corpus-lab/node-fonts.mjs
// (the system font index of the corpus lab and its provider) over the generated font folder
// (cargo run -p uo-fonts --example provider_corpus) plus shared/fonts/files, driven by the system
// scenarios (`S`) of tests/parity/provider_scenarios.txt. Writes tests/parity/provider_oracle.cap,
// replayed by tests/provider_parity.rs (`oracle_policy_matches_node_fonts`).
//
//   UO_TS_ROOT=<built checkout of main> node tests/parity_support/capture_system.mjs --corpus <folder> [--trace <file>]
//
// The roots are exactly the two folders (WINDIR and LOCALAPPDATA point nowhere). Per scenario
// (`oracle/<id>`): the stream of the answer lines, one provider for the whole scenario. The line is
//   key|family|sha256|bold italic synthBold synthItalic standIn|identity
// with the path of the key reduced to the file name and identity the index of the first answer of
// the scenario that is the same object. `oracle/families`: the family list, sorted.
import { execSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Stream, importFrom, tsRoot } from '../../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = tsRoot();
const argv = process.argv.slice(2);
const arg = (name) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : null);
const corpus = arg('--corpus');
const traceFile = arg('--trace');
if (!corpus) throw new Error('--corpus <folder> is required');

process.env.WINDIR = 'Z:/uo-fonts-nowhere';
process.env.LOCALAPPDATA = 'Z:/uo-fonts-nowhere';
process.env.EXTRA_FONT_DIRS = `${corpus};${join(root, 'shared/fonts/files')}`;
const { createNodeFontProvider } = await importFrom(root, 'write/tools/corpus-lab/node-fonts.mjs');

const sha = (u8) => createHash('sha256').update(u8).digest('hex');
const normalizeKey = (key) => {
  const at = key.lastIndexOf('#');
  if (at < 0) return key;
  const head = key.slice(0, at), tail = key.slice(at + 1);
  const m = /^([^|]*)\|([^|]*)\|(.*)$/s.exec(head);
  if (!m) return key;
  return `${m[1]}|${m[2]}|${m[3].split(/[\\/]/).pop()}#${tail}`;
};

const lines = readFileSync(join(here, '..', 'parity', 'provider_scenarios.txt'), 'utf8').split('\n').filter(Boolean).map((l) => l.split('\t'));
const scenarios = [];
for (let i = 0; i < lines.length; i++) {
  if (lines[i][0] !== 'S') continue;
  const asks = [];
  let j = i + 1;
  while (lines[j][0] !== 'E') asks.push(lines[j++]);
  scenarios.push({ id: lines[i][1], asks });
  i = j;
}

const cap = new CaptureWriter(['the `oracle` provider policy: createNodeFontProvider of write/tools/corpus-lab/node-fonts.mjs; digests only']);
cap.meta('ts-source', 'write/tools/corpus-lab/node-fonts.mjs');
cap.meta('ts-commit', execSync('git rev-parse --short=8 HEAD', { cwd: root }).toString().trim());
cap.meta('inputs', `${scenarios.length} scenarios`);
const trace = [];

const families = (await createNodeFontProvider()).families().sort();
cap.set('oracle/families', 'list', `${families.length}:${createHash('sha256').update(families.join('\u0001')).digest('hex').slice(0, 32)}`);
trace.push(`families ${families.join('|')}`);

for (const sc of scenarios) {
  const provider = await createNodeFontProvider();
  const s = new Stream();
  const seen = [];
  trace.push(`## ${sc.id}`);
  for (const [, family, b, i] of sc.asks) {
    const a = provider.face(family, b === '1', i === '1');
    let line = '-';
    if (a) {
      let at = seen.indexOf(a);
      if (at < 0) { seen.push(a); at = seen.length - 1; }
      const f = (x) => (x ? 1 : 0);
      line = `${normalizeKey(a.key)}|${a.family}|${sha(a.bytes)}|${f(a.bold)}${f(a.italic)}${f(a.synthBold)}${f(a.synthItalic)}${f(a.standIn)}|${at}`;
    }
    s.tag('q').str(line);
    trace.push(`s ${family} ${b}${i} ${line}`);
  }
  const d = s.finish();
  cap.set(`oracle/${sc.id}`, 'stream', `${s.items}:${d.short}`);
}

cap.write(join(here, '..', 'parity', 'provider_oracle.cap'));
if (traceFile) writeFileSync(traceFile, trace.join('\n') + '\n');
console.log(`provider_oracle.cap: ${scenarios.length} scenarios, ${families.length} families`);
