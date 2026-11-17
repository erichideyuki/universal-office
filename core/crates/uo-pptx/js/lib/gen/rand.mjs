// Seeded randomness for the deck generators: the xorshift32 of uo-testkit (`Rng`), mixed so that
// consecutive seeds give unrelated streams, with the small helpers the generators use. No Date,
// no Math.random anywhere in the generators: same seed -> same bytes.
import { Rng } from '../../../../uo-testkit/js/testkit.mjs';

export class Rand {
  constructor(seed) {
    this.r = new Rng(((Math.imul(seed >>> 0, 0x9e3779b1) ^ 0x5bd1e995) >>> 0) || 1);
    for (let i = 0; i < 16; i++) this.r.next();
  }
  /** uniform integer in [0, n) */
  below(n) { return this.r.below(n); }
  /** uniform integer in [a, b] */
  int(a, b) { return a + this.r.below(b - a + 1); }
  /** true with probability p (0..1, resolution 1/1000) */
  chance(p) { return this.r.below(1000) < Math.round(p * 1000); }
  pick(list) { return list[this.r.below(list.length)]; }
  /** `n` distinct picks (n clamped to the list length), in pick order */
  pickN(list, n) {
    const pool = [...list];
    const out = [];
    for (let i = 0; i < n && pool.length; i++) out.push(pool.splice(this.r.below(pool.length), 1)[0]);
    return out;
  }
  /** weighted pick: items [[value, weight], ...] */
  weighted(items) {
    let total = 0;
    for (const [, w] of items) total += w;
    let t = this.r.below(total);
    for (const [v, w] of items) { if (t < w) return v; t -= w; }
    return items[items.length - 1][0];
  }
  hex6() { return (this.r.next() & 0xffffff).toString(16).toUpperCase().padStart(6, '0'); }
  guid() {
    const h = (n) => Array.from({ length: n }, () => this.r.below(16).toString(16).toUpperCase()).join('');
    return `{${h(8)}-${h(4)}-4${h(3)}-${'89AB'[this.r.below(4)]}${h(3)}-${h(12)}}`;
  }
}
