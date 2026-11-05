//! `PVec<T>`: a persistent chunked vector of `Arc<T>` (architecture A2, A3, U19): the paragraphs
//! and table anchors of a story. Cloning copies only the chunk pointers (O(n/64)), so an undo
//! snapshot shares every chunk it did not touch; a write goes through copy-on-write on the
//! chunk (`Arc::make_mut`) and on the element. The layout uses `Arc::ptr_eq` of an element as its
//! "content unchanged" fast path, never as identity (identity is the explicit id, G22).
//!
//! Public because `Story` exposes it to uo-docx-layout (read API); the history module of
//! milestone 2 (`uo_docx::history`, G5) is built on it.

use std::sync::Arc;

/// Elements per chunk after a split (chunks hold 1 to 2 × this).
const CHUNK: usize = 64;

/// A persistent vector of `Arc<T>` in chunks (see the module comment).
pub struct PVec<T> {
    chunks: Vec<Arc<Vec<Arc<T>>>>,
    /// index of the first element of each chunk
    starts: Vec<usize>,
    len: usize,
}

impl<T> Clone for PVec<T> {
    fn clone(&self) -> Self {
        PVec { chunks: self.chunks.clone(), starts: self.starts.clone(), len: self.len }
    }
}

impl<T> Default for PVec<T> {
    fn default() -> Self {
        PVec { chunks: Vec::new(), starts: Vec::new(), len: 0 }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for PVec<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: PartialEq> PartialEq for PVec<T> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.iter().zip(other.iter()).all(|(a, b)| a == b)
    }
}

impl<T> FromIterator<T> for PVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::from_arcs(iter.into_iter().map(Arc::new))
    }
}

impl<T> PVec<T> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// From shared elements (no copy of the elements).
    pub fn from_arcs(iter: impl IntoIterator<Item = Arc<T>>) -> Self {
        let mut v = PVec::new();
        let mut cur: Vec<Arc<T>> = Vec::new();
        for x in iter {
            cur.push(x);
            if cur.len() == CHUNK {
                v.push_chunk(std::mem::take(&mut cur));
            }
        }
        if !cur.is_empty() {
            v.push_chunk(cur);
        }
        v
    }

    fn push_chunk(&mut self, c: Vec<Arc<T>>) {
        self.starts.push(self.len);
        self.len += c.len();
        self.chunks.push(Arc::new(c));
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// (chunk, offset) of element `i`.
    fn locate(&self, i: usize) -> Option<(usize, usize)> {
        if i >= self.len {
            return None;
        }
        let c = self.starts.partition_point(|&s| s <= i).checked_sub(1)?;
        Some((c, i - self.starts.get(c)?))
    }

    pub fn get(&self, i: usize) -> Option<&T> {
        self.get_arc(i).map(|a| &**a)
    }

    /// The shared element (for `Arc::ptr_eq` "unchanged" checks and sharing into another vector).
    pub fn get_arc(&self, i: usize) -> Option<&Arc<T>> {
        let (c, k) = self.locate(i)?;
        self.chunks.get(c)?.get(k)
    }

    pub fn first(&self) -> Option<&T> {
        self.get(0)
    }

    pub fn last(&self) -> Option<&T> {
        self.len.checked_sub(1).and_then(|i| self.get(i))
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + '_ {
        self.chunks.iter().flat_map(|c| c.iter().map(|a| &**a))
    }

    pub fn iter_arcs(&self) -> impl DoubleEndedIterator<Item = &Arc<T>> + '_ {
        self.chunks.iter().flat_map(|c| c.iter())
    }

    pub fn push(&mut self, x: T) {
        self.push_arc(Arc::new(x));
    }

    pub fn push_arc(&mut self, x: Arc<T>) {
        match self.chunks.last_mut() {
            Some(c) if c.len() < 2 * CHUNK => {
                Arc::make_mut(c).push(x);
                self.len += 1;
            }
            _ => self.push_chunk(vec![x]),
        }
    }

    /// Replaces element `i` (a new object in the TS sense: the caller gives it a new id when the
    /// TS builds a new object, G22). Returns the old element.
    pub fn replace(&mut self, i: usize, x: Arc<T>) -> Option<Arc<T>> {
        let (c, k) = self.locate(i)?;
        let slot = Arc::make_mut(self.chunks.get_mut(c)?).get_mut(k)?;
        Some(std::mem::replace(slot, x))
    }

    /// Inserts at `i` (`i == len` appends); `false` when `i > len`.
    pub fn insert(&mut self, i: usize, x: Arc<T>) -> bool {
        if i == self.len {
            self.push_arc(x);
            return true;
        }
        let Some((c, k)) = self.locate(i) else { return false };
        let Some(chunk) = self.chunks.get_mut(c) else { return false };
        let v = Arc::make_mut(chunk);
        v.insert(k, x);
        if v.len() > 2 * CHUNK {
            let tail = v.split_off(v.len() / 2);
            self.chunks.insert(c + 1, Arc::new(tail));
        }
        self.len += 1;
        self.reindex(c);
        true
    }

    /// Removes element `i`.
    pub fn remove(&mut self, i: usize) -> Option<Arc<T>> {
        let (c, k) = self.locate(i)?;
        let chunk = self.chunks.get_mut(c)?;
        let v = Arc::make_mut(chunk);
        if k >= v.len() {
            return None;
        }
        let x = v.remove(k);
        if v.is_empty() {
            self.chunks.remove(c);
        }
        self.len -= 1;
        self.reindex(c);
        Some(x)
    }

    fn reindex(&mut self, from: usize) {
        self.starts.truncate(from);
        let mut s = match from.checked_sub(1) {
            Some(p) => self.starts.get(p).copied().unwrap_or(0) + self.chunks.get(p).map_or(0, |c| c.len()),
            None => 0,
        };
        for c in self.chunks.iter().skip(from) {
            self.starts.push(s);
            s += c.len();
        }
    }

    /// How many chunks this vector shares (by pointer) with `other` (history step cost, U19).
    #[must_use]
    pub fn shared_chunks(&self, other: &PVec<T>) -> usize {
        self.chunks.iter().filter(|c| other.chunks.iter().any(|o| Arc::ptr_eq(c, o))).count()
    }

    /// Number of chunks.
    #[must_use]
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// The elements of the chunks of `self` that `other` does not share by pointer, with their
    /// index (the session's history step cost, U19; O(chunks²) pointer comparisons).
    pub fn unshared_chunk_elements<'s>(&'s self, other: &'s PVec<T>) -> impl Iterator<Item = (usize, &'s Arc<T>)> + 's {
        self.chunks
            .iter()
            .zip(self.starts.iter())
            .filter(move |(c, _)| !other.chunks.iter().any(|o| Arc::ptr_eq(c, o)))
            .flat_map(|(c, &start)| c.iter().enumerate().map(move |(k, x)| (start + k, x)))
    }
}

impl<T: Clone> PVec<T> {
    /// Element `i` for an in-place mutation (the TS mutates the object: the id stays, G22);
    /// copy-on-write on the chunk and on the element.
    pub fn make_mut(&mut self, i: usize) -> Option<&mut T> {
        let (c, k) = self.locate(i)?;
        let slot = Arc::make_mut(self.chunks.get_mut(c)?).get_mut(k)?;
        Some(Arc::make_mut(slot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_remove_and_sharing() {
        let mut v: PVec<u32> = (0..300).collect();
        assert_eq!(v.len(), 300);
        assert_eq!(v.get(299), Some(&299));
        let snap = v.clone();
        *v.make_mut(5).unwrap() = 1000;
        assert_eq!(snap.get(5), Some(&5));
        assert_eq!(v.get(5), Some(&1000));
        assert_eq!(v.shared_chunks(&snap), v.chunk_count() - 1);
        for i in 0..200 {
            assert!(v.insert(10, Arc::new(i)));
        }
        assert_eq!(v.len(), 500);
        assert_eq!(v.get(10), Some(&199));
        assert_eq!(v.get(209), Some(&0));
        assert_eq!(v.get(210), Some(&10));
        for _ in 0..200 {
            v.remove(10);
        }
        assert_eq!(v.iter().copied().collect::<Vec<_>>(), (0..300).map(|i| if i == 5 { 1000 } else { i }).collect::<Vec<_>>());
        while v.remove(0).is_some() {}
        assert!(v.is_empty());
        v.push(7);
        assert_eq!(v.first(), Some(&7));
    }
}
