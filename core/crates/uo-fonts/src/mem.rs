//! Estimates of the heap a structure holds, for `Face::heap_bytes` (the registry's LRU counts
//! `bytes.len() + heap_bytes()`). Capacities, not allocator statistics: the figure is what the
//! structures asked for, without the allocator's own headers.

use std::mem::size_of;

/// The control bytes after the buckets of a hashbrown table (one group: 16 on SSE2, 8 elsewhere).
const GROUP: usize = if cfg!(all(target_arch = "x86_64", target_feature = "sse2")) { 16 } else { 8 };

/// The bytes of a hashbrown table (`FxHashMap`, `FxHashSet`) of `capacity` usable slots holding
/// entries of type `T` (`(K, V)` for a map, `K` for a set): the buckets of the entries, one control
/// byte per bucket and the trailing group. 0 for a table that never allocated.
pub(crate) fn table_bytes<T>(capacity: usize) -> usize {
    if capacity == 0 {
        return 0;
    }
    let buckets = if capacity < 8 { capacity + 1 } else { capacity / 7 * 8 };
    let data = (buckets * size_of::<T>()).next_multiple_of(GROUP);
    data + buckets + GROUP
}

/// The bytes of a vector's buffer.
pub(crate) fn vec_bytes<T>(capacity: usize) -> usize {
    capacity * size_of::<T>()
}
