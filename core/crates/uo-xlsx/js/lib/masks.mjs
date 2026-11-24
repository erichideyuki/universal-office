// Prints the 8x8 pattern-fill masks of `grid-layout.ts` `PATTERN_MASKS` as Rust rows (one-off helper for
// `src/grid/paint_rules/fill.rs`; `tests/paint_parity.rs` compares the Rust table with the TS bundle's).
const rows = (fn) => {
  const out = [];
  for (let y = 0; y < 8; y++) {
    let m = 0;
    for (let x = 0; x < 8; x++) if (fn(x, y)) m |= 1 << (7 - x);
    out.push(m);
  }
  return out;
};
const lightGray = (x, y) => (y % 2 === 0 ? x % 4 === 0 : x % 4 === 2);
const gridU = (v) => (v + 1) % 4 < 2;
const M = {
  gray0625: rows((x, y) => (y % 4 === 0 && x % 8 === 0) || (y % 4 === 2 && x % 8 === 4)),
  gray125: rows((x, y) => (y % 4 === 0 && x % 4 === 0) || (y % 4 === 2 && x % 4 === 2)),
  lightGray: rows(lightGray),
  mediumGray: rows((x, y) => (x + y) % 2 === 0),
  darkGray: rows((x, y) => !(y % 2 === 0 ? x % 4 === 3 : x % 4 === 1)),
  darkHorizontal: rows((_x, y) => y % 4 < 2),
  lightHorizontal: rows((_x, y) => y % 4 === 0),
  darkVertical: rows((x) => x % 4 < 2),
  lightVertical: rows((x) => x % 4 === 0),
  darkDown: rows((x, y) => (x - y + 8) % 4 < 2),
  lightDown: rows((x, y) => (x - y + 8) % 4 === 0),
  darkUp: rows((x, y) => (x + y) % 4 >= 2),
  lightUp: rows((x, y) => (x + y) % 4 === 3),
  darkGrid: rows((x, y) => gridU(x) === gridU(y)),
  lightGrid: rows((x, y) => x % 4 === 0 || y % 4 === 0),
  darkTrellis: rows((x, y) => y % 2 === 0 || (y % 4 === 1 ? x % 4 === 1 || x % 4 === 2 : x % 4 === 0 || x % 4 === 3)),
  lightTrellis: rows((x, y) => (y % 2 === 1 && x % 2 === 1) || (y % 4 === 0 && x % 4 === 0) || (y % 4 === 2 && x % 4 === 2)),
};
for (const [k, v] of Object.entries(M)) console.log(`    ("${k}", [${v.map((n) => `0x${n.toString(16).padStart(2, '0')}`).join(', ')}]),`);
