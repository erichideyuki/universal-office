// wasm32 stack of the A2 recursion (record core/docs/modules/uo-calc.md "A2 — as built", decision A2-2): loads the
// `eval_stack_wasm` example and, for each scenario and depth, paints the shadow stack below the probe address with
// two patterns, runs one phase (print, evaluate) and finds the lowest byte the phase touched. Fails when the deepest
// accepted nesting (MAX_AST_DEPTH) or the evaluation guard (MAX_EVAL_DEPTH, the `lambda` scenario) needs more than
// the budget of the 1 MiB shadow stack.
//   cargo build -p uo-calc --example eval_stack_wasm --target wasm32-unknown-unknown --release
//   node core/crates/uo-calc/js/wasm-stack.mjs <target>/wasm32-unknown-unknown/release/examples/eval_stack_wasm.wasm [levels...]
import { readFileSync } from 'node:fs';

const BUDGET = 512 * 1024;
const path = process.argv[2];
if (!path) throw new Error('usage: node wasm-stack.mjs <eval_stack_wasm.wasm> [levels...]');
const levels = process.argv.slice(3).map(Number);
const DEFAULT_LEVELS = [1, 64, 256, 511, 512, 513, 100000];
const KINDS = ['paren', 'power', 'let', 'chain', 'unary', 'lambda', 'concat'];
const { instance } = await WebAssembly.instantiate(readFileSync(path), {});
const w = instance.exports;

function stackDepth(setup, call) {
  const sp = w.eval_stack_probe();
  const span = sp - 1024;
  let depth = 0;
  let result;
  for (const pattern of [0xa5, 0x5a]) {
    setup();
    new Uint8Array(w.memory.buffer, sp - span, span).fill(pattern);
    result = call();
    const after = new Uint8Array(w.memory.buffer, sp - span, span);
    let lowest = 0;
    while (lowest < span && after[lowest] === pattern) lowest++;
    if (lowest < 4096) throw new Error('the call used more than the painted stack');
    depth = Math.max(depth, span - lowest);
  }
  return { depth, result };
}

let worst = 0;
for (let k = 0; k < KINDS.length; k++) {
  for (const n of levels.length ? levels : DEFAULT_LEVELS) {
    const row = [];
    let parsed;
    for (const phase of [1, 2]) {
      const setup = () => { w.eval_stack_phase(k, n, 3); parsed = w.eval_stack_phase(k, n, 0); };
      const { depth, result } = stackDepth(setup, () => w.eval_stack_phase(k, n, phase));
      row.push(`${phase === 1 ? 'print' : 'eval'} ${String(depth).padStart(7)} B${phase === 2 ? ` (${(depth / n).toFixed(0)} B/level, result ${result})` : ''}`);
      worst = Math.max(worst, depth);
    }
    w.eval_stack_phase(k, n, 3);
    console.log(`${KINDS[k].padEnd(6)} ${String(n).padStart(6)} (parsed ${parsed === 1}): ${row.join(', ')}`);
  }
}
console.log(`worst: ${worst} B of the 1 MiB shadow stack (budget ${BUDGET} B)`);
if (worst > BUDGET) throw new Error(`the A2 recursion needs ${worst} B of shadow stack, over the budget of ${BUDGET} B`);
