// wasm32 stack of nested stories (G20, record "Nested-story stack — fixed"): loads the
// `nested_stack_wasm` example and, for each depth, paints the shadow stack below the probe
// address with two patterns, runs one phase (read, digest, drop of a text box nested that deep)
// and finds the lowest byte the phase touched. Fails when 64 levels need more than the budget.
//   cargo build -p uo-docx --example nested_stack_wasm --target wasm32-unknown-unknown --profile wasm
//   node core/crates/uo-docx/js/wasm-stack.mjs <target>/wasm32-unknown-unknown/wasm/examples/nested_stack_wasm.wasm
import { readFileSync } from 'node:fs';

const BUDGET = 256 * 1024;
const path = process.argv[2];
if (!path) throw new Error('usage: node wasm-stack.mjs <nested_stack_wasm.wasm> [levels...]');
const levels = process.argv.slice(3).map(Number);
const DEFAULT_LEVELS = [0, 1, 8, 64, 65];
const { instance } = await WebAssembly.instantiate(readFileSync(path), {});
const w = instance.exports;

// the lowest byte below the probe address that `call` touched, over two patterns
function stackDepth(setup, call) {
  const sp = w.nested_stack_probe();
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

const phases = ['read', 'digest', 'drop'];
let worst = 0;
const KINDS = ['vml', 'dml', 'dml-group', 'vml-group', 'alternate', 'mixed'];
// UO_NESTED_BLOCKS=0 leaves out the block reader of the full document (a build without it)
const MODES = process.env.UO_NESTED_BLOCKS === '0' ? [0, 1] : [0, 1, 2, 3];
for (const mode of KINDS.flatMap((_, k) => MODES.map((m) => (k << 2) | m))) {
  for (const n of levels.length ? levels : DEFAULT_LEVELS) {
    const row = [];
    for (let phase = 0; phase < 3; phase++) {
      // the read starts from nothing, the digest and the drop from a fresh read
      const setup = () => {
        w.nested_stack_phase(n, mode, 2);
        if (phase > 0) w.nested_stack_phase(n, mode, 0);
      };
      const { depth, result } = stackDepth(setup, () => w.nested_stack_phase(n, mode, phase));
      if (phase === 0 && result !== 1) throw new Error(`the read of ${n} levels failed`);
      row.push(`${phases[phase]} ${depth} B`);
      if (n <= 65) worst = Math.max(worst, depth);
    }
    w.nested_stack_phase(n, mode, 2);
    const tags = [KINDS[mode >> 2], mode & 2 ? 'blocks' : 'paragraphs', mode & 1 ? 'sources' : ''].filter(Boolean).join(', ');
    console.log(`levels ${String(n).padStart(3)} (${tags}): ${row.join(', ')}`);
  }
}
console.log(`worst at <= 65 levels: ${worst} B of the 1 MiB shadow stack (budget ${BUDGET} B)`);
if (worst > BUDGET) throw new Error(`nested stories need ${worst} B of shadow stack, over the budget of ${BUDGET} B`);
