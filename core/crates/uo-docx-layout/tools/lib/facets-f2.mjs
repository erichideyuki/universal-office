// P3 facets of frontier F2 (the Word semantics in `uo_docx::word`; record "Wave 1 — F2"),
// recorded by facets.mjs during every layout of capture-layout.mjs, with their inputs dumped for
// the Rust replay (tests/f2_parity.rs):
//
//   p3.datefmt    every `dateFieldValue(instr, now, lang)` (datefmt.ts:87): tag 'd', u16s instr,
//                 num now.getTime(), num the local offset (minutes east, -getTimezoneOffset),
//                 u16s lang, u16s result;
//   p3.formula    every `evaluateFieldFormula(code, value, lang)` (field-formula.ts:36, nested
//                 calls included): tag 'f', u16s code, optU16s lang, u32 n, then per answer of
//                 the nested-field callback u16s name, u16s instr, optU16s answer, then optU16s
//                 result;
//   p3.numbering  every `NumberingState.next(numId, ilvl)` (numbering.ts:164) of the pass's
//                 states (labels by first appearance; `clone()` recorded as 'c' src dst, a state
//                 first seen as 'n' label + its language): tag 'x', u32 label, the two arguments
//                 (`a` = tag 'u' undefined | 'z' null | 'n' num), then the marker's canonical digest
//                 (uo-docx canon.mjs) as u16s, or "null";
//   p3.style      the first call per (method, id) of `StyleResolver.paraStyle`/`charStyle`/
//                 `tableStyle` (resolve.ts:193–222; pure over the formatting): tag 's', u16s
//                 method, optU16s id, u16s the canonical digest of the result;
//   p3.hyphlangs  `documentHyphenationLanguages(full)` and with `force` (hyphenate.ts:65), once
//                 per layout: u16s the JSON of each list.
//
// p3.numbering and p3.style replay over the formatting the D1a readers give (R2); p3.hyphlangs
// needs the whole document (R5). The dump `<key>.f2.tsv` (tab-separated, strings `-` = undefined,
// `~` = null, else `:` + the UTF-16 units in hex, 4 digits each; numbers as String(n)):
//   D <instr> <t> <offset> <lang>                       a dateFieldValue call
//   F <code> <lang> <n> (<name> <instr> <answer>)*      an evaluateFieldFormula call
//   C <src> <dst>                                        NumberingState clone
//   N <label> <lang>                                     a state first seen
//   X <label> <numId> <ilvl>                             a next call (numbers as above, `-`/`~`)
//   S <method> <id>                                      a style resolver query
//   H <json>                                             the p3.hyphlangs lists
import { Stream } from '../../../uo-testkit/js/testkit.mjs';
import { Canon } from '../../../uo-docx/js/canon.mjs';
import { Sink } from './sink.mjs';

/** Module functions F2 wraps (merged into facets.mjs WRAPS): the pre-hook rewires the formula's
 *  nested-field callback; `layoutDocument` gives the document to the hyphenation walk. */
export const F2_WRAPS = { 'datefmt.js': ['dateFieldValue'], 'field-formula.js': ['evaluateFieldFormula'], 'engine.js': ['layoutDocument'] };

const TAB = String.fromCharCode(9), NL = String.fromCharCode(10);
const WRAPPED = Symbol('f2-wrapped');
const hex16 = (s) => Array.from({ length: s.length }, (_, i) => s.charCodeAt(i).toString(16).padStart(4, '0')).join('');
const enc = (s) => (s === undefined ? '-' : s === null ? '~' : ':' + hex16(String(s)));
const encNum = (n) => (n === undefined ? '-' : n === null ? '~' : Object.is(n, -0) ? '-0' : String(n));
const digest = (v) => {
  const s = new Stream();
  new Canon(s).value(v, 'x');
  return `${s.items}:${s.finish().short}`;
};

export function f2Recorder({ NumberingState, StyleResolver, documentHyphenationLanguages }) {
  let st = null;
  const origNext = NumberingState.prototype.next, origClone = NumberingState.prototype.clone;
  const origPara = StyleResolver.prototype.paraStyle, origChar = StyleResolver.prototype.charStyle, origTable = StyleResolver.prototype.tableStyle;
  const label = (ns) => {
    let l = st.labels.get(ns);
    if (l === undefined) {
      st.labels.set(ns, (l = st.labels.size));
      st.numbering.tag('n').u32(l).u16s(ns.lang);
      st.dump.push(['N', l, enc(ns.lang)].join(TAB));
    }
    return l;
  };
  const arg = (s, v) => (v === undefined ? s.tag('u') : v === null ? s.tag('z') : s.tag('n').num(v));
  function next(numId, ilvl) {
    if (!st) return origNext.call(this, numId, ilvl);
    const l = label(this);
    const m = origNext.call(this, numId, ilvl);
    const s = st.numbering.tag('x').u32(l);
    arg(s, numId); arg(s, ilvl);
    s.u16s(m ? digest(m) : 'null');
    st.dump.push(['X', l, encNum(numId), encNum(ilvl)].join(TAB));
    return m;
  }
  function clone() {
    const c = origClone.call(this);
    if (st) {
      const src = label(this);
      st.labels.set(c, st.labels.size);
      const dst = st.labels.get(c);
      st.numbering.tag('c').u32(src).u32(dst);
      st.dump.push(['C', src, dst].join(TAB));
    }
    return c;
  }
  const style = (method, orig) => function (id) {
    const r = orig.call(this, id);
    if (st) {
      const k = `${method}|${id === undefined ? '-' : id}`;
      if (!st.styleSeen.has(k)) {
        st.styleSeen.add(k);
        st.style.tag('s').u16s(method).optU16s(id).u16s(digest(r));
        st.dump.push(['S', method, enc(id)].join(TAB));
      }
    }
    return r;
  };
  return {
    start() {
      st = { datefmt: new Sink(), formula: new Sink(), numbering: new Sink(), style: new Sink(), hyph: null, labels: new WeakMap(), styleSeen: new Set(), dump: [], calls: [] };
      // labels in first-appearance order: a WeakMap with a counter
      let n = 0;
      const wm = new WeakMap();
      st.labels = { get: (o) => wm.get(o), set: (o, v) => { wm.set(o, v); n++; }, get size() { return n; } };
      NumberingState.prototype.next = next;
      NumberingState.prototype.clone = clone;
      StyleResolver.prototype.paraStyle = style('paraStyle', origPara);
      StyleResolver.prototype.charStyle = style('charStyle', origChar);
      StyleResolver.prototype.tableStyle = style('tableStyle', origTable);
    },
    /** Before a wrapped call: the formula's callback recorded per call (nested calls included). */
    pre(name, args) {
      if (!st) return;
      if (name === 'layoutDocument' && st.hyph === null) {
        const full = args[0];
        st.hyph = JSON.stringify([documentHyphenationLanguages(full), documentHyphenationLanguages(full, 'pt-BR', true)]);
      }
      if (name === 'evaluateFieldFormula') {
        st.calls.push({ answers: [] });
        // the outermost call wraps the callback once (nested calls receive the wrapper); every
        // open call sees each answer (an outer call re-runs its inner ones in the replay)
        const value = args[1];
        if (typeof value === 'function' && !value[WRAPPED]) {
          const w = (n, instr) => {
            const a = value(n, instr);
            if (st) for (const c of st.calls) c.answers.push([n, instr, a]);
            return a;
          };
          w[WRAPPED] = true;
          args[1] = w;
        }
      }
    },
    report(name, args, r) {
      if (!st) return;
      if (name === 'dateFieldValue') {
        const [instr, now, lang] = args;
        const off = -now.getTimezoneOffset();
        st.datefmt.tag('d').u16s(instr).num(now.getTime()).num(off).u16s(lang ?? 'pt-BR').u16s(r);
        st.dump.push(['D', enc(instr), encNum(now.getTime()), encNum(off), enc(lang)].join(TAB));
      } else if (name === 'evaluateFieldFormula') {
        const call = st.calls.pop();
        const [code, , lang] = args;
        const s = st.formula.tag('f').u16s(code).optU16s(lang).u32(call.answers.length);
        for (const [n, i, a] of call.answers) s.u16s(n).u16s(i).optU16s(a);
        s.optU16s(r);
        st.dump.push(['F', enc(code), enc(lang), call.answers.length, ...call.answers.flatMap(([n, i, a]) => [enc(n), enc(i), enc(a)])].join(TAB));
      }
    },
    finish() {
      NumberingState.prototype.next = origNext;
      NumberingState.prototype.clone = origClone;
      StyleResolver.prototype.paraStyle = origPara;
      StyleResolver.prototype.charStyle = origChar;
      StyleResolver.prototype.tableStyle = origTable;
      const facets = {
        'p3.datefmt': st.datefmt.finish().value,
        'p3.formula': st.formula.finish().value,
        'p3.numbering': st.numbering.finish().value,
        'p3.style': st.style.finish().value,
        'p3.hyphlangs': new Sink().u16s(st.hyph ?? 'null').finish().value,
      };
      st.dump.push(['H', enc(st.hyph ?? 'null')].join(TAB));
      const dump = st.dump.join(NL) + NL;
      st = null;
      return { facets, dump };
    },
  };
}
