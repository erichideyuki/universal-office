//! The JS object operations the Word semantics are written in, over any [`crate::js_object!`]
//! type, with the key insertion order kept (K9, LQ20: `JSON.stringify` keys and equality observe
//! it):
//!
//! - [`spread`] = `{ ...base, ...over }`, [`spread_opt`] = `{ ...base?, ...over? }`;
//! - [`assign`] = `Object.assign(target, over)`;
//! - [`set`] = `o.k = v` (an existing key keeps its place, a new key goes last);
//! - [`delete`] = `delete o.k`;
//! - [`json_eq`] = `JSON.stringify(a) === JSON.stringify(b)` (`eq`, context.ts:217);
//! - [`js_sort`] = `Array.prototype.sort` with a comparator, as V8's TimSort decides it (the
//!   comparator result `< 0` or not), so a comparator that answers NaN orders as in V8.
//!
//! A key that holds `undefined` (the cleared bit) is a key: the spread copies it, and it keeps its
//! place.

use crate::js::{JsAssign, JsObject, JsValue, JsonWriter, KeyOrder, Slot};

/// Whether key `i` exists (a value or `undefined`).
#[must_use]
pub fn has_key<T: JsObject>(o: &T, i: usize) -> bool {
    !matches!(o.slot(i), Slot::Absent)
}

/// The indices of the keys of `o`, in insertion order.
#[must_use]
pub fn key_indices<T: JsObject>(o: &T) -> Vec<u8> {
    let keys = T::keys();
    let mut out = Vec::new();
    o.for_each_in_order(&mut |k, _| {
        if let Some(i) = keys.iter().position(|n| *n == k).and_then(|i| u8::try_from(i).ok()) {
            out.push(i);
        }
    });
    out
}

/// Records `order` as the key order of `o` (no allocation when it is the declaration order).
pub fn set_key_order<T: JsObject>(o: &mut T, order: &[u8]) {
    let mut declared = true;
    let mut last: Option<u8> = None;
    for &i in order {
        if last.is_some_and(|l| l >= i) {
            declared = false;
            break;
        }
        last = Some(i);
    }
    let mut k = KeyOrder::default();
    if !declared {
        for &i in order {
            k.touch(i);
        }
    }
    *o.key_order_mut() = k;
}

/// `Object.assign(target, over)`: every key of `over` in its order; a key new to `target` goes
/// last, an existing one keeps its place.
pub fn assign<T: JsAssign>(target: &mut T, over: &T) {
    let mut order = key_indices(target);
    let mut grew = false;
    for i in key_indices(over) {
        let i_us = usize::from(i);
        if !has_key(target, i_us) {
            order.push(i);
            grew = true;
        }
        target.assign_slot(over, i_us);
    }
    if grew {
        set_key_order(target, &order);
    }
}

/// `{ ...base, ...over }`.
#[must_use]
pub fn spread<T: JsAssign + Clone>(base: &T, over: &T) -> T {
    let mut out = base.clone();
    assign(&mut out, over);
    out
}

/// `{ ...base, ...over }` where either may be `undefined` (spreading `undefined` adds nothing).
#[must_use]
pub fn spread_opt<T: JsAssign + Clone + Default>(base: Option<&T>, over: Option<&T>) -> T {
    let mut out = base.cloned().unwrap_or_default();
    if let Some(o) = over {
        assign(&mut out, o);
    }
    out
}

/// `o.k = …`: `write` stores the value (an `Option` field set to `None` must also set the
/// cleared bit to stay a key: use [`set_undefined`]); a key new to `o` goes last.
pub fn set<T: JsObject>(o: &mut T, key: &str, write: impl FnOnce(&mut T)) {
    let Some(i) = T::key_index(key) else {
        write(o);
        return;
    };
    let i_us = usize::from(i);
    if has_key(o, i_us) {
        write(o);
        return;
    }
    let mut order = key_indices(o);
    write(o);
    if has_key(o, i_us) {
        order.push(i);
        set_key_order(o, &order);
    }
}

/// `o.k = undefined` (`write` clears the field to `None`): the key exists with `undefined`.
pub fn set_undefined<T: JsObject>(o: &mut T, key: &str, write: impl FnOnce(&mut T)) {
    let Some(i) = T::key_index(key) else {
        write(o);
        return;
    };
    let i_us = usize::from(i);
    let had = has_key(o, i_us);
    let order = if had { Vec::new() } else { key_indices(o) };
    write(o);
    o.cleared_mut().set(u32::from(i));
    if !had {
        let mut order = order;
        order.push(i);
        set_key_order(o, &order);
    }
}

/// `delete o.k`.
pub fn delete<T: JsAssign>(o: &mut T, key: &str) {
    if let Some(i) = T::key_index(key) {
        o.remove_slot(usize::from(i));
        o.key_order_mut().forget(i);
    }
}

/// `JSON.stringify(v)` (`None` = `undefined`).
#[must_use]
pub fn json_of(v: Option<&dyn JsValue>) -> Option<String> {
    v.and_then(|v| JsonWriter::stringify(v, None))
}

/// `JSON.stringify(a) === JSON.stringify(b)` (`eq`, context.ts:217; revisions.ts:111, 122).
#[must_use]
pub fn json_eq(a: Option<&dyn JsValue>, b: Option<&dyn JsValue>) -> bool {
    json_of(a) == json_of(b)
}

/// `arr.sort((a, b) => cmp(a, b))` as V8 runs it (third_party/v8/builtins/array-sort.tq): one
/// run counted and extended by binary insertion below 64 elements, the decision being
/// `cmp(…) < 0` (so NaN counts as "not less"). From 64 elements on, the merge phase is a stable
/// merge sort on the same decision, which equals V8 whenever the comparator is consistent
/// (declared: a NaN-answering comparator over 64 or more elements may order differently from V8;
/// it never panics, unlike `slice::sort_by` on an inconsistent order).
pub fn js_sort<T>(v: &mut [T], cmp: impl Fn(&T, &T) -> f64) {
    uo_jsnum::sort::js_sort_by(v, cmp);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::props::{IndSpec, RFonts};

    fn json(v: &dyn JsValue) -> String {
        JsonWriter::stringify(v, None).unwrap_or_default()
    }

    #[test]
    fn spread_keeps_base_order_then_new_keys() {
        let mut a = IndSpec { right: Some(2.0.into()), ..IndSpec::default() };
        a.mark("right");
        let mut b = IndSpec { left: Some(1.0.into()), right: Some(3.0.into()), ..IndSpec::default() };
        b.mark("left");
        b.mark("right");
        let c = spread(&a, &b);
        assert_eq!(json(&c), r#"{"right":3,"left":1}"#);
    }

    #[test]
    fn delete_then_set_goes_last() {
        let mut f = RFonts { ascii: Some("A".into()), h_ansi: Some("B".into()), ..RFonts::default() };
        delete(&mut f, "ascii");
        set(&mut f, "ascii", |f| f.ascii = Some("C".into()));
        assert_eq!(json(&f), r#"{"hAnsi":"B","ascii":"C"}"#);
    }

    #[test]
    fn undefined_overrides_in_spread() {
        let a = RFonts { ascii: Some("A".into()), ..RFonts::default() };
        let mut b = RFonts::default();
        set_undefined(&mut b, "ascii", |b| b.ascii = None);
        let c = spread(&a, &b);
        assert_eq!(json(&c), "{}");
        assert!(has_key(&c, 0));
    }

    #[test]
    fn js_sort_is_stable_and_takes_nan_as_not_less() {
        // V8 (Node 24.15) on the same tagged pairs: "beacd" and "afcbde"
        let mut v = vec![(3.0, 'a'), (1.0, 'b'), (3.0, 'c'), (f64::NAN, 'd'), (2.0, 'e')];
        js_sort(&mut v, |a, b| a.0 - b.0);
        assert_eq!(v.iter().map(|x| x.1).collect::<String>(), "beacd");
        let mut w = vec![(f64::NAN, 'a'), (1.0, 'b'), (0.0, 'c'), (f64::NAN, 'd'), (2.0, 'e'), (-1.0, 'f')];
        js_sort(&mut w, |a, b| a.0 - b.0);
        assert_eq!(w.iter().map(|x| x.1).collect::<String>(), "afcbde");
    }
}
