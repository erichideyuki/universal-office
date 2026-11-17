//! The source of `Math.random()` of the editor (`freshCreationId` is its only consumer): a seeded
//! xorshift32 (the generator of the parity harnesses, `uo-testkit`'s `Rng`), so a session is
//! reproducible. The wasm host seeds it from its own entropy ([`crate::edit::Editor::set_random_seed`]).

/// Marsaglia xorshift32 with the 13/17/5 triple; the state is never zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Xorshift32(u32);

impl Xorshift32 {
    /// A generator from a seed (0 becomes 1).
    pub fn new(seed: u32) -> Xorshift32 {
        Xorshift32(if seed == 0 { 1 } else { seed })
    }

    /// The next value in `[0, 1)`: `state / 2^32`.
    pub fn next_f64(&mut self) -> f64 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        f64::from(s) / 4_294_967_296.0
    }
}

impl Default for Xorshift32 {
    fn default() -> Self {
        Xorshift32::new(1)
    }
}
