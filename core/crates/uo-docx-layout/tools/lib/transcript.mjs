// P2, the font provider transcript (record §5, K2a, K4, LQ19): every `provider.face(family,
// bold, italic)` the engine makes and its answer, **with the answer's identity** ("same object
// as answer j": kerning, ligatures and run merging apply only across the same FontFace object).
// The Rust replay answers from it (the registry's transcript policy), so the engine side is
// compared independently of any provider policy (those have their own facets, P2b).
//
// Facets:
//   p2       every call in order: u16s(family) bool(bold) bool(italic) then i32-ish u32 answer
//            label (0 = null, j + 1 = answer j); a new answer also writes its description;
//   p2first  the first request per lower-case key `family|bold|italic` with its answer label
//            (what Rust must reproduce: it resolves once per layout call, the TS asks again in
//            every pass, LQ19).
// The transcript file (`--dump`, `transcriptText()`) lists the answers with their bytes digest and
// source, for the Rust replay to load the same faces (a digest that differs fails the gate: LQ21).
// Line format (tab-separated, every string through encodeURIComponent; tests/common/mod.rs reads it):
//   A <label> <key> <family> <bold> <italic> <synthBold> <synthItalic> <standIn> <sha256|-> <len|-> <path|embed|-> <index|->
//   C <family> <bold> <italic> <answer label, 0 = null>
import { createHash } from 'node:crypto';
import { Sink } from './sink.mjs';

const sha = (u8) => createHash('sha256').update(u8).digest('hex');

export function recordProvider(provider) {
  const answers = []; // { obj, desc }
  const ids = new Map();
  const calls = [];
  const wrapped = {
    face(family, bold, italic) {
      const out = provider.face(family, bold, italic);
      let label = 0;
      if (out) {
        let j = ids.get(out);
        if (j === undefined) {
          j = answers.length;
          ids.set(out, j);
          const m = /\|([^|]*)#(\d+)$/.exec(out.key ?? '');
          answers.push({
            key: out.key, family: out.family, bold: !!out.bold, italic: !!out.italic,
            synthBold: !!out.synthBold, synthItalic: !!out.synthItalic, standIn: !!out.standIn,
            bytes: out.bytes ? { len: out.bytes.length, sha256: sha(out.bytes) } : null,
            source: (out.key ?? '').startsWith('embed:') ? { embedded: true } : m ? { path: m[1], index: Number(m[2]) } : null,
          });
        }
        label = j + 1;
      }
      calls.push({ family, bold: !!bold, italic: !!italic, answer: label });
      return out;
    },
  };
  const facets = () => {
    const all = new Sink(), first = new Sink();
    const described = new Set(), firstKeys = new Set();
    const ans = (s, label, seen) => {
      s.u32(label);
      if (label && !seen.has(label)) {
        seen.add(label);
        const a = answers[label - 1];
        s.u16s(a.key).u16s(a.family).bool(a.bold).bool(a.italic).bool(a.synthBold).bool(a.synthItalic).bool(a.standIn);
        s.bool(!!a.bytes); if (a.bytes) s.u16s(a.bytes.sha256);
      }
    };
    const seenFirst = new Set();
    for (const c of calls) {
      all.u16s(c.family).bool(c.bold).bool(c.italic); ans(all, c.answer, described);
      const k = `${c.family.toLowerCase()}|${c.bold}|${c.italic}`;
      if (firstKeys.has(k)) continue;
      firstKeys.add(k);
      first.u16s(k); ans(first, c.answer, seenFirst);
    }
    return { p2: all.finish().value, p2first: first.finish().value };
  };
  const transcriptText = () => {
    const TAB = String.fromCharCode(9), NL = String.fromCharCode(10);
    const e = encodeURIComponent, b = (v) => (v ? '1' : '0');
    const lines = answers.map((a, j) => ['A', j + 1, e(a.key ?? ''), e(a.family ?? ''), b(a.bold), b(a.italic), b(a.synthBold), b(a.synthItalic), b(a.standIn),
      a.bytes ? a.bytes.sha256 : '-', a.bytes ? a.bytes.len : '-', a.source?.embedded ? 'embed' : a.source?.path ? e(a.source.path) : '-', a.source?.index ?? '-'].join(TAB));
    for (const c of calls) lines.push(['C', e(c.family), b(c.bold), b(c.italic), c.answer].join(TAB));
    return lines.join(NL) + NL;
  };
  return { provider: wrapped, calls, answers, facets, transcriptText };
}
