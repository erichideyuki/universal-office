//! TS plain objects used as dictionaries (`Record<string, T>`) and TS `Map`s.

use indexmap::IndexMap;
use uo_jsnum::object::array_index;

use super::{JsValue, JsonWriter, Str};

/// A TS plain object used as a dictionary (`Record<string, T>`, `Record<number, T>`): string keys
/// in JS own-key order — array-index keys (`"0"` … `"4294967294"`) first, ascending, then the
/// others in insertion order (ECMA-262 OrdinaryOwnPropertyKeys; uo-jsnum `object`). Assigning an
/// existing key keeps its place; removing one is `delete`.
#[derive(Debug, Clone, PartialEq)]
pub struct JsRecord<V> {
    map: IndexMap<Str, V>,
    /// how many leading entries are array indices
    ints: usize,
}

impl<V> Default for JsRecord<V> {
    fn default() -> Self {
        JsRecord { map: IndexMap::new(), ints: 0 }
    }
}

impl<V> JsRecord<V> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// `o[key]` (own keys only, as the TS `own()` helper and `Object.hasOwn`).
    pub fn get(&self, key: &str) -> Option<&V> {
        self.map.get(key)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        self.map.get_mut(key)
    }

    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    /// `o[key] = value`; returns the previous value.
    pub fn insert(&mut self, key: &str, value: V) -> Option<V> {
        if let Some(v) = self.map.get_mut(key) {
            return Some(std::mem::replace(v, value));
        }
        match array_index(key) {
            Some(n) => {
                let at = self
                    .map
                    .get_range(..self.ints)
                    .map_or(0, |ints| ints.keys().take_while(|k| array_index(k).is_some_and(|m| m < n)).count());
                self.map.shift_insert(at, key.into(), value);
                self.ints += 1;
            }
            None => {
                self.map.insert(key.into(), value);
            }
        }
        None
    }

    /// `delete o[key]`.
    pub fn remove(&mut self, key: &str) -> Option<V> {
        let (i, _, v) = self.map.shift_remove_full(key)?;
        if i < self.ints {
            self.ints -= 1;
        }
        Some(v)
    }

    /// Entries in JS own-key order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &V)> + '_ {
        self.map.iter().map(|(k, v)| (&**k, v))
    }

    pub fn keys(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.map.keys().map(|k| &**k)
    }

    pub fn values(&self) -> impl ExactSizeIterator<Item = &V> + '_ {
        self.map.values()
    }

    /// The values for an in-place change (keys and their order unchanged; the session's undo
    /// normalisation, D2a).
    pub fn values_mut(&mut self) -> impl ExactSizeIterator<Item = &mut V> + '_ {
        self.map.values_mut()
    }
}

impl<V: JsValue> JsValue for JsRecord<V> {
    /// Canonical form: an object with the keys sorted (A6), as every plain object.
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.order_facet(|f| {
            for k in self.keys() {
                f(k);
            }
        });
        let mut entries: Vec<(&str, &V)> = self.iter().collect();
        entries.sort_by(|a, b| crate::canon::utf16_cmp(a.0, b.0));
        w.object(entries.len());
        for (k, v) in entries {
            w.key(k);
            v.canon(w);
        }
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.begin_object();
        for (k, v) in self.iter() {
            w.member(k, v);
        }
        w.end_object();
        true
    }
    /// The entries in their stored order (finer than the JS own-key order, which is derived from it).
    fn exact(&self, k: &mut super::ExactKey) -> bool {
        k.tag(b'r') && k.count(self.map.len()) && self.map.iter().all(|(key, v)| k.str(key) && v.exact(k))
    }
}

/// A TS `Map`: insertion order (re-setting a key keeps its place; `delete` then `set` moves it to
/// the end).
pub type JsMap<K, V> = IndexMap<K, V>;

impl<K: JsValue, V: JsValue> JsValue for IndexMap<K, V> {
    /// Canonical form: a map, entries in iteration order (A6).
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.map(self.len());
        for (k, v) in self {
            k.canon(w);
            v.canon(w);
        }
    }
    /// `JSON.stringify(new Map(…))` is `{}`.
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.raw("{}");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_keys_first_ascending() {
        let mut r = JsRecord::new();
        r.insert("b", 1);
        r.insert("10", 2);
        r.insert("a", 3);
        r.insert("2", 4);
        r.insert("01", 5);
        r.insert("b", 6);
        assert_eq!(r.keys().collect::<Vec<_>>(), ["2", "10", "b", "a", "01"]);
        assert_eq!(r.get("b"), Some(&6));
        r.remove("2");
        r.insert("0", 7);
        assert_eq!(r.keys().collect::<Vec<_>>(), ["0", "10", "b", "a", "01"]);
    }
}
