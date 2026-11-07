//! TS objects: the field bookkeeping and the [`crate::js_object!`] declaration macro.

use super::{ExactKey, JsValue, JsonWriter};

/// One bit per field: "the TS object has this key with the value `undefined`" (A2, U4: what
/// `Object.assign(props, patch)` with a cleared field leaves). A set bit with a `None` field is an
/// explicit `undefined`; with `Some` the bit is meaningless and ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldMask(u64);

impl FieldMask {
    #[must_use]
    pub fn has(self, i: u32) -> bool {
        i < 64 && self.0 & (1 << i) != 0
    }

    pub fn set(&mut self, i: u32) {
        if i < 64 {
            self.0 |= 1 << i;
        }
    }

    pub fn clear(&mut self, i: u32) {
        if i < 64 {
            self.0 &= !(1 << i);
        }
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// `{ ...base, ...over }` of the masks: a key cleared in `over` stays cleared (an `over`
    /// field present wins over both; the caller handles the values).
    #[must_use]
    pub fn bits(self) -> u64 {
        self.0
    }
}

/// The insertion order of an object's keys (K9, LQ20), as indices into its `KEYS`, when it is not
/// the declaration order: the keys recorded here come first (in this order), then every other
/// present key in declaration order. A reader whose order is data-dependent records **every**
/// key it assigns (`JsObject::mark` after each assignment), so the recorded list is the whole
/// order. Empty (no allocation) for objects built in declaration
/// order, which is what a reader assigning in a fixed code order gives when the struct declares
/// its fields in that order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyOrder(Box<[u8]>);

impl KeyOrder {
    /// Records key `i` as inserted now, unless it already has a position (re-assigning a key
    /// keeps its place, as in JS).
    pub fn touch(&mut self, i: u8) {
        if !self.0.contains(&i) {
            let mut v = std::mem::take(&mut self.0).into_vec();
            v.push(i);
            self.0 = v.into_boxed_slice();
        }
    }

    /// Forgets key `i` (`delete o.k`: a later assignment puts it at the end).
    pub fn forget(&mut self, i: u8) {
        if self.0.contains(&i) {
            self.0 = self.0.iter().copied().filter(|&k| k != i).collect();
        }
    }

    /// The recorded indices, in insertion order.
    #[must_use]
    pub fn recorded(&self) -> &[u8] {
        &self.0
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// What a TS object holds under one key.
#[derive(Clone, Copy)]
pub enum Slot<'a> {
    /// no such own key
    Absent,
    /// the key exists with the value `undefined`
    Undefined,
    /// a value
    Val(&'a dyn JsValue),
}

/// A TS object declared with [`crate::js_object!`]: its keys and what each holds.
pub trait JsObject {
    /// The TS key names, in declaration order (index = field index, cleared-bit index).
    fn keys() -> &'static [&'static str];
    /// The key indices sorted by name in UTF-16 code-unit order (the canonical digest order).
    fn sorted_keys() -> &'static [u8];
    /// What key `i` holds.
    fn slot(&self, i: usize) -> Slot<'_>;
    fn key_order(&self) -> &KeyOrder;
    fn key_order_mut(&mut self) -> &mut KeyOrder;
    fn cleared_mut(&mut self) -> &mut FieldMask;

    /// The index of a TS key name (`None` for a name the type does not have).
    fn key_index(name: &str) -> Option<u8> {
        Self::keys().iter().position(|k| *k == name).and_then(|i| u8::try_from(i).ok())
    }

    /// Records that key `name` was assigned now (data-dependent insertion orders, item 4 of the
    /// frontier declarations). Unknown names are ignored.
    fn mark(&mut self, name: &str) {
        if let Some(i) = Self::key_index(name) {
            self.key_order_mut().touch(i);
        }
    }

    /// Records `o[name] = undefined` (the field must be `None`): the cleared bit. The key's
    /// position follows the same rule as any key (declaration order unless [`JsObject::mark`]ed).
    fn mark_undefined(&mut self, name: &str) {
        if let Some(i) = Self::key_index(name) {
            self.cleared_mut().set(u32::from(i));
        }
    }

    /// The own keys in insertion order with their slots (see [`JsObject::for_each_in_order`]).
    fn slots_in_order(&self) -> Vec<(&'static str, Slot<'_>)> {
        let keys = Self::keys();
        let recorded = self.key_order().recorded();
        let mut out = Vec::new();
        for &i in recorded {
            let i = usize::from(i);
            if let (Some(k), s) = (keys.get(i), self.slot(i))
                && !matches!(s, Slot::Absent)
            {
                out.push((*k, s));
            }
        }
        for (i, k) in keys.iter().enumerate() {
            if recorded.iter().any(|&r| usize::from(r) == i) {
                continue;
            }
            let s = self.slot(i);
            if !matches!(s, Slot::Absent) {
                out.push((*k, s));
            }
        }
        out
    }

    /// The own keys in insertion order (`Object.keys` for non-integer names), each with its slot.
    fn for_each_in_order(&self, f: &mut dyn FnMut(&'static str, Slot<'_>)) {
        let keys = Self::keys();
        let recorded = self.key_order().recorded();
        for &i in recorded {
            let i = usize::from(i);
            if let (Some(k), s) = (keys.get(i), self.slot(i))
                && !matches!(s, Slot::Absent)
            {
                f(k, s);
            }
        }
        for (i, k) in keys.iter().enumerate() {
            if recorded.iter().any(|&r| usize::from(r) == i) {
                continue;
            }
            let s = self.slot(i);
            if !matches!(s, Slot::Absent) {
                f(k, s);
            }
        }
    }
}

/// Byte-wise `a < b` (TS names are ASCII, where this is the UTF-16 code-unit order).
const fn str_lt(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut i = 0;
    while i < a.len() && i < b.len() {
        if a[i] != b[i] {
            return a[i] < b[i];
        }
        i += 1;
    }
    a.len() < b.len()
}

/// The indices of `names` sorted by name (compile time, for [`JsObject::sorted_keys`]).
#[must_use]
pub const fn sort_keys<const N: usize>(names: &[&str]) -> [u8; N] {
    let mut out = [0u8; N];
    let mut i = 0;
    while i < N {
        out[i] = i as u8;
        i += 1;
    }
    let mut i = 1;
    while i < N {
        let mut j = i;
        while j > 0 && str_lt(names[out[j] as usize], names[out[j - 1] as usize]) {
            let t = out[j];
            out[j] = out[j - 1];
            out[j - 1] = t;
            j -= 1;
        }
        i += 1;
    }
    out
}

/// The canonical form of a declared object: `{` with the present keys in sorted order (A6).
#[cfg(feature = "canon")]
#[doc(hidden)]
pub fn object_canon<T: JsObject>(o: &T, w: &mut crate::canon::CanonWriter<'_>) {
    w.order_facet(|f| o.for_each_in_order(&mut |k, _| f(k)));
    let sorted = T::sorted_keys();
    let n = sorted.iter().filter(|&&i| !matches!(o.slot(usize::from(i)), Slot::Absent)).count();
    w.object(n);
    for &i in sorted {
        let i = usize::from(i);
        match o.slot(i) {
            Slot::Absent => {}
            Slot::Undefined => {
                w.key(T::keys().get(i).copied().unwrap_or(""));
                w.undefined();
            }
            Slot::Val(v) => {
                w.key(T::keys().get(i).copied().unwrap_or(""));
                v.canon(w);
            }
        }
    }
}

/// Whether a declared object has no Rust-only fields (`hidden { … }` of [`crate::js_object!`]): only
/// then is its exact identity its TS keys.
#[doc(hidden)]
#[must_use]
pub const fn no_hidden_fields(hidden: &[&str]) -> bool {
    hidden.is_empty()
}

/// The start of the exact identity of a declared object ([`JsValue::exact`]): the recorded key
/// order and the whole cleared mask. The macro then writes the presence of every key (one bit per
/// field) and the exact key of each value present, in declaration order.
#[doc(hidden)]
pub fn object_exact_head(keys: &KeyOrder, cleared: FieldMask, k: &mut ExactKey) -> bool {
    let order = keys.recorded();
    k.tag(b'O') && k.count(order.len()) && order.iter().all(|&i| k.tag(i)) && k.u64(cleared.bits())
}

/// The object-safe view of a declared object: the JSON walker exists once, each type only has this vtable.
pub trait JsObjectDyn {
    /// [`JsObject::keys`].
    fn js_keys(&self) -> &'static [&'static str];
    /// [`JsObject::slot`].
    fn js_slot(&self, i: usize) -> Slot<'_>;
    /// [`JsObject::key_order`].
    fn js_key_order(&self) -> &KeyOrder;
}

impl<T: JsObject> JsObjectDyn for T {
    fn js_keys(&self) -> &'static [&'static str] {
        T::keys()
    }
    fn js_slot(&self, i: usize) -> Slot<'_> {
        self.slot(i)
    }
    fn js_key_order(&self) -> &KeyOrder {
        self.key_order()
    }
}

/// [`JsObject::for_each_in_order`] over the view (same body).
#[inline(never)]
fn for_each_in_order_dyn<'a>(o: &'a dyn JsObjectDyn, f: &mut dyn FnMut(&'static str, Slot<'a>)) {
    let keys = o.js_keys();
    let recorded = o.js_key_order().recorded();
    for &i in recorded {
        let i = usize::from(i);
        if let (Some(k), s) = (keys.get(i), o.js_slot(i))
            && !matches!(s, Slot::Absent)
        {
            f(k, s);
        }
    }
    for (i, k) in keys.iter().enumerate() {
        if recorded.iter().any(|&r| usize::from(r) == i) {
            continue;
        }
        let s = o.js_slot(i);
        if !matches!(s, Slot::Absent) {
            f(k, s);
        }
    }
}

/// `JSON.stringify` of a declared object: keys in insertion order, `undefined` omitted.
#[doc(hidden)]
#[inline(never)]
pub fn object_json(o: &dyn JsObjectDyn, w: &mut JsonWriter<'_>) {
    if w.sorted_keys() {
        object_json_with(o, &[], &[], w);
        return;
    }
    w.begin_object();
    for_each_in_order_dyn(o, &mut |k, s| {
        if let Slot::Val(v) = s {
            w.member(k, v);
        }
    });
    w.end_object();
}

/// The canonical form of a declared object plus extra entries (hand-written types: stories,
/// aliased views, flattened rare fields): all keys sorted together. `extra` is in insertion
/// order and goes after the object's keys in the order facet.
#[cfg(feature = "canon")]
pub fn object_canon_with<T: JsObject>(o: &T, extra: &[crate::canon::EntryRef<'_>], w: &mut crate::canon::CanonWriter<'_>) {
    w.order_facet(|f| {
        o.for_each_in_order(&mut |k, _| f(k));
        for (k, _) in extra {
            f(k);
        }
    });
    enum E<'a, 'b> {
        Slot(Slot<'a>),
        Extra(&'b crate::canon::ValueFn<'b>),
    }
    let mut all: Vec<(&str, E<'_, '_>)> = Vec::new();
    for (i, k) in T::keys().iter().enumerate() {
        let s = o.slot(i);
        if !matches!(s, Slot::Absent) {
            all.push((k, E::Slot(s)));
        }
    }
    for (k, f) in extra {
        all.push((k, E::Extra(*f)));
    }
    all.sort_by(|a, b| crate::canon::utf16_cmp(a.0, b.0));
    w.object(all.len());
    for (k, e) in all {
        w.key(k);
        match e {
            E::Slot(Slot::Val(v)) => v.canon(w),
            E::Slot(_) => w.undefined(),
            E::Extra(f) => f(w),
        }
    }
}

/// The members of a signature object in UTF-16 code-unit order of their names (G23): one sort for every type.
#[inline(never)]
fn sort_members(all: &mut [(&str, &dyn JsValue)]) {
    all.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
}

/// `JSON.stringify` of a declared object with extra members: `before`, the object's keys in
/// insertion order, then `after`.
#[inline(never)]
pub fn object_json_with(o: &dyn JsObjectDyn, before: &[(&str, &dyn JsValue)], after: &[(&str, &dyn JsValue)], w: &mut JsonWriter<'_>) {
    if w.sorted_keys() {
        // the order-insensitive signature encoding (D1b, G23): every member sorted by name
        let mut all: Vec<(&str, &dyn JsValue)> = before.to_vec();
        for_each_in_order_dyn(o, &mut |k, s| {
            if let Slot::Val(v) = s {
                all.push((k, v));
            }
        });
        all.extend_from_slice(after);
        sort_members(&mut all);
        w.begin_object();
        for (k, v) in all {
            w.member(k, v);
        }
        w.end_object();
        return;
    }
    w.begin_object();
    for (k, v) in before {
        w.member(k, *v);
    }
    for_each_in_order_dyn(o, &mut |k, s| {
        if let Slot::Val(v) = s {
            w.member(k, v);
        }
    });
    for (k, v) in after {
        w.member(k, *v);
    }
    w.end_object();
}

#[doc(hidden)]
#[macro_export]
macro_rules! __js_ty {
    (opt $ty:ty) => { ::core::option::Option<$ty> };
    (req $ty:ty) => { $ty };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __js_slot {
    (opt, $s:expr, $f:ident, $i:expr) => {
        match &$s.$f {
            ::core::option::Option::Some(v) => $crate::js::Slot::Val(v),
            ::core::option::Option::None if $s.cleared.has($i) => $crate::js::Slot::Undefined,
            ::core::option::Option::None => $crate::js::Slot::Absent,
        }
    };
    (req, $s:expr, $f:ident, $i:expr) => {
        $crate::js::Slot::Val(&$s.$f)
    };
}

/// Declares a TS object of the model.
///
/// ```ignore
/// js_object! {
///     /// docs
///     pub struct ColorSpec {
///         /// RRGGBB or `auto`
///         opt val: Str = "val",
///         opt theme_color: Str = "themeColor",
///     }
///     hidden { pub id: ParaId }   // optional: Rust-only fields, not TS keys
/// }
/// ```
///
/// `opt` fields are `Option<T>` (`k?: T`), `req` fields are `T`. The struct also gets
/// `pub cleared: FieldMask` and `pub keys: KeyOrder`, derives `Debug, Clone, Default, PartialEq`,
/// and implements [`JsObject`] and [`JsValue`] (canonical digest under the TS names, sorted;
/// `JSON.stringify` in insertion order). The field order of the declaration is the default key
/// order: declare fields in the order the reader assigns them.
#[macro_export]
macro_rules! js_object {
    (custom_value: $($rest:tt)*) => { $crate::js_object!(@decl custom; $($rest)*); };
    (@decl $value:ident;
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$fmeta:meta])*
                $kind:ident $field:ident : $ty:ty = $ts:literal
            ),* $(,)?
        }
        $( hidden { $( $(#[$hmeta:meta])* $hvis:vis $hfield:ident : $hty:ty ),* $(,)? } )?
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Default, PartialEq)]
        $vis struct $name {
            $( $(#[$fmeta])* pub $field: $crate::__js_ty!($kind $ty), )*
            /// keys that exist with the value `undefined` (one bit per field)
            pub cleared: $crate::js::FieldMask,
            /// the insertion order of the keys, when not the declaration order
            pub keys: $crate::js::KeyOrder,
            $( $( $(#[$hmeta])* $hvis $hfield: $hty, )* )?
        }

        const _: () = {
            #[allow(non_camel_case_types, dead_code, clippy::upper_case_acronyms)]
            enum K { $( $field ),* }
            const NAMES: &[&str] = &[ $( $ts ),* ];
            const N: usize = NAMES.len();
            assert!(N <= 64, "a TS object of the model has at most 64 fields (FieldMask)");
            const SORTED: [u8; N] = $crate::js::sort_keys::<N>(NAMES);
            $(
                #[allow(non_snake_case)]
                fn $field(s: &$name) -> $crate::js::Slot<'_> {
                    $crate::__js_slot!($kind, s, $field, K::$field as u32)
                }
            )*
            const SLOTS: &[for<'a> fn(&'a $name) -> $crate::js::Slot<'a>] = &[ $( $field ),* ];

            impl $crate::js::JsObject for $name {
                fn keys() -> &'static [&'static str] { NAMES }
                fn sorted_keys() -> &'static [u8] { &SORTED }
                fn slot(&self, i: usize) -> $crate::js::Slot<'_> {
                    SLOTS.get(i).map_or($crate::js::Slot::Absent, |f| f(self))
                }
                fn key_order(&self) -> &$crate::js::KeyOrder { &self.keys }
                fn key_order_mut(&mut self) -> &mut $crate::js::KeyOrder { &mut self.keys }
                fn cleared_mut(&mut self) -> &mut $crate::js::FieldMask { &mut self.cleared }
            }

            // Rust-only fields are not TS keys: a type with any is not internable (`JsValue::exact`)
            #[allow(dead_code, reason = "unused by the `custom_value:` types, which write their own `JsValue`")]
            const EXACT: bool = $crate::js::no_hidden_fields(&[ $( $( stringify!($hfield) ),* )? ]);
            #[allow(dead_code, reason = "unused by the `custom_value:` types, which write their own `JsValue`")]
            fn __exact_fields(s: &$name, k: &mut $crate::js::ExactKey) -> bool {
                let mut present = 0u64;
                $( present |= u64::from($crate::__js_present!($kind, s, $field)) << (K::$field as u32); )*
                k.u64(present) $( && $crate::__js_exact_field!($kind, s, $field, k) )*
            }
            $crate::__js_value_impl!($value $name, EXACT, __exact_fields);

            // `JsAssign` (uo-docx-layout F2): typed per-key copy and removal, so that
            // `{ ...a, ...b }`, `Object.assign` and `delete` can be written once over any object
            const ASSIGN: &[fn(&mut $name, &$name)] = &[ $( |d: &mut $name, s: &$name| { $crate::__js_assign!($kind, d, s, $field, K::$field as u32) } ),* ];
            const REMOVE: &[fn(&mut $name)] = &[ $( |d: &mut $name| { $crate::__js_remove!($kind, d, $field, K::$field as u32) } ),* ];

            impl $crate::js::JsAssign for $name {
                fn assign_slot(&mut self, from: &Self, i: usize) {
                    if let Some(f) = ASSIGN.get(i) {
                        f(self, from);
                    }
                }
                fn remove_slot(&mut self, i: usize) {
                    if let Some(f) = REMOVE.get(i) {
                        f(self);
                    }
                }
            }

            // the undo normalisation of the session (D2a, A3 "Restore"): what the TS JSON round
            // trip of a history step does to this object and everything below it
            #[cfg(feature = "session")]
            impl $crate::session::normalize::JsNormalize for $name {
                fn js_needs(&self) -> bool {
                    !self.cleared.is_empty()
                        $( || $crate::session::normalize::JsNormalize::js_needs(&self.$field) )*
                        $( $( || $crate::session::normalize::JsNormalize::js_needs(&self.$hfield) )* )?
                }
                fn js_normalize(&mut self, cx: &mut $crate::session::normalize::NormCx<'_>) {
                    $crate::session::normalize::drop_cleared(&mut self.cleared, &mut self.keys);
                    $( $crate::session::normalize::JsNormalize::js_normalize(&mut self.$field, cx); )*
                    $( $( $crate::session::normalize::JsNormalize::js_normalize(&mut self.$hfield, cx); )* )?
                }
            }
        };
    };
    ($($rest:tt)*) => { $crate::js_object!(@decl auto; $($rest)*); };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __js_assign {
    (opt, $d:expr, $s:expr, $f:ident, $i:expr) => {{
        $d.$f = $s.$f.clone();
        if $s.$f.is_none() && $s.cleared.has($i) {
            $d.cleared.set($i);
        } else {
            $d.cleared.clear($i);
        }
    }};
    (req, $d:expr, $s:expr, $f:ident, $i:expr) => {{
        $d.$f = $s.$f.clone();
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __js_remove {
    (opt, $d:expr, $f:ident, $i:expr) => {{
        $d.$f = ::core::option::Option::None;
        $d.cleared.clear($i);
    }};
    (req, $d:expr, $f:ident, $i:expr) => {{
        let _ = &$d;
    }};
}

/// Typed per-key access of a [`crate::js_object!`] type, generated by the macro (uo-docx-layout
/// F2): what `{ ...a, ...b }`, `Object.assign` and `delete` do to one key. The key order is the
/// caller's business (`uo_docx::word::obj` keeps it).
pub trait JsAssign: JsObject {
    /// `self[k] = from[k]` for key `i` as `from` holds it: a value is cloned, an explicit
    /// `undefined` stays one; an absent key in `from` makes key `i` absent (callers only copy keys
    /// `from` has).
    fn assign_slot(&mut self, from: &Self, i: usize);
    /// `delete self[k]` for key `i` (required fields cannot be absent: no-op).
    fn remove_slot(&mut self, i: usize);
}

#[doc(hidden)]
#[macro_export]
macro_rules! __js_present {
    (opt, $s:expr, $f:ident) => {
        $s.$f.is_some()
    };
    (req, $s:expr, $f:ident) => {
        true
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __js_exact_field {
    (opt, $s:expr, $f:ident, $k:expr) => {
        match &$s.$f {
            ::core::option::Option::Some(v) => $crate::js::JsValue::exact(v, $k),
            ::core::option::Option::None => true,
        }
    };
    (req, $s:expr, $f:ident, $k:expr) => {
        $crate::js::JsValue::exact(&$s.$f, $k)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __js_value_impl {
    (custom $name:ident $(, $exact:expr, $fields:ident)?) => {};
    (auto $name:ident) => {
        impl $crate::js::JsValue for $name {
            #[cfg(feature = "canon")]
            fn canon(&self, w: &mut $crate::canon::CanonWriter<'_>) {
                $crate::js::object_canon(self, w);
            }
            fn json(&self, w: &mut $crate::js::JsonWriter<'_>) -> bool {
                $crate::js::object_json(self, w);
                true
            }
        }
    };
    (auto $name:ident, $exact:expr, $fields:ident) => {
        impl $crate::js::JsValue for $name {
            #[cfg(feature = "canon")]
            fn canon(&self, w: &mut $crate::canon::CanonWriter<'_>) {
                $crate::js::object_canon(self, w);
            }
            fn json(&self, w: &mut $crate::js::JsonWriter<'_>) -> bool {
                $crate::js::object_json(self, w);
                true
            }
            fn exact(&self, k: &mut $crate::js::ExactKey) -> bool {
                $exact && $crate::js::object_exact_head(&self.keys, self.cleared, k) && $fields(self, k)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_names_at_compile_time() {
        const S: [u8; 4] = sort_keys::<4>(&["val", "themeColor", "b", "bCs"]);
        assert_eq!(S, [2, 3, 1, 0]);
    }

    #[test]
    fn key_order_touch_and_forget() {
        let mut k = KeyOrder::default();
        k.touch(3);
        k.touch(1);
        k.touch(3);
        assert_eq!(k.recorded(), &[3, 1]);
        k.forget(3);
        k.touch(3);
        assert_eq!(k.recorded(), &[1, 3]);
    }
}
