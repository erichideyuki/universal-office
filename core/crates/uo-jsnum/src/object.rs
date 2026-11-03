//! The key order of a JavaScript object: the order of `Object.keys`, `Object.entries` and
//! `for…in` over an ordinary object built by inserting its keys one after another. Array-index
//! keys come first in ascending numeric order, then every other string key in insertion order
//! (ECMA-262 OrdinaryOwnPropertyKeys). Engines that mirror TS code keeping maps in plain objects
//! (zip entries, XML attributes) need it.

/// The value of `key` when it is an array index: `"0"` to `"4294967294"` in canonical form (no
/// sign, no leading zero, nothing but ASCII digits); `None` for every other key.
#[must_use]
pub fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || key.len() > 10 || !key.bytes().all(|b| b.is_ascii_digit()) || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    key.parse::<u64>().ok().filter(|&v| v < 4_294_967_295).map(|v| v as u32)
}

/// Orders `(key, value)` pairs like the keys of a JS object built by inserting them in this
/// order: the array indices first, ascending, then the rest in insertion order. Returns the
/// input untouched when no key is an array index.
#[must_use]
pub fn js_object_order<T>(items: Vec<(String, T)>) -> Vec<(String, T)> {
    if !items.iter().any(|(n, _)| array_index(n).is_some()) {
        return items;
    }
    let (mut ints, rest): (Vec<_>, Vec<_>) = items.into_iter().partition(|(n, _)| array_index(n).is_some());
    ints.sort_by_key(|(n, _)| array_index(n));
    ints.extend(rest);
    ints
}
