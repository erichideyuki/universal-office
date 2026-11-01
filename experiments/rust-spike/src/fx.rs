//! Hash rápido para chaves inteiras (o FxHash do rustc), em vez do SipHash padrão — o que
//! um código Rust de verdade usaria para mapas de glifos. Sem dependência externa.
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Default, Clone, Copy)]
pub struct FxHasher(u64);

impl Hasher for FxHasher {
    /// a rotação final do rustc-hash 2: sem ela os bits baixos (os que o hashbrown usa para o
    /// balde) dependem só dos bits baixos da chave — g1·65536 + g2 empilha por g2
    #[inline]
    fn finish(&self) -> u64 {
        self.0.rotate_left(26)
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.write_u64(i as u64);
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
}

pub type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;
pub type FxSet<K> = HashSet<K, BuildHasherDefault<FxHasher>>;
