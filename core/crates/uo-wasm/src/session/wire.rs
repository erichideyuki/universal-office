//! The conversions between the session's wire types (`codec::generated`, from `schema/session.uos`)
//! and uo-docx's types: the traits the generated `codec::bind` implements per bound type, and the
//! leaf impls it relies on (primitives, strings, lists, options, boxes, ids, numbers, maps).
//!
//! Declared narrowings (record "Milestone 2: the session in wasm", WS-D2): `i64`, `i8` and `i16` travel
//! as `i32` (out of range = [`BindError::Range`]), `usize` and `u64` as `u32`; `JsNumber::Null` goes out
//! as NaN and never comes back as `null`; `String`/`Str` arguments are built from UTF-16 with a lone
//! surrogate replaced by U+FFFD (`Utf16` and `Vec<u16>` keep the units).

use std::sync::Arc;

use uo_docx::ids::{DrawingId, HfPartId, MediaId, ParaId, SectionBreakId, TableId, TxbxSrc};
use uo_docx::js::{JsNumber, JsRecord, NumKey};
use uo_docx::session::JobId;
use uo_docx::session::VersionId;
use uo_docx::text::Utf16;

/// Why a wire value has no native form (or the other way).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    /// a number outside the native type's range
    Range(&'static str),
    /// a variant only the session produces (`FormatCmd::ApplyPainter`, `ClipboardCmd::RenderPictures`)
    NativeOnly(&'static str),
    /// a value the native type cannot hold
    Invalid(&'static str),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::Range(w) => write!(f, "out of range: {w}"),
            BindError::NativeOnly(w) => write!(f, "not a wire value: {w}"),
            BindError::Invalid(w) => write!(f, "invalid: {w}"),
        }
    }
}

/// A native value from its wire form.
pub trait FromWire<W>: Sized {
    /// # Errors
    /// [`BindError`] when the wire value has no native form.
    fn from_wire(w: W) -> Result<Self, BindError>;
}

/// The wire form of a native value.
pub trait IntoWire<W> {
    /// # Errors
    /// [`BindError`] when the native value has no wire form.
    #[allow(clippy::wrong_self_convention, reason = "the value stays the session's: the wire form is a copy")]
    fn into_wire(&self) -> Result<W, BindError>;
}

// ───────────────────────────── references and boxes ─────────────────────────────

impl<T: IntoWire<W> + ?Sized, W> IntoWire<W> for &T {
    fn into_wire(&self) -> Result<W, BindError> {
        (**self).into_wire()
    }
}

impl<T: FromWire<W>, W> FromWire<W> for Box<T> {
    fn from_wire(w: W) -> Result<Self, BindError> {
        Ok(Box::new(T::from_wire(w)?))
    }
}

impl<T: IntoWire<W> + ?Sized, W> IntoWire<W> for Box<T> {
    fn into_wire(&self) -> Result<W, BindError> {
        (**self).into_wire()
    }
}

impl<T: FromWire<W>, W> FromWire<W> for Arc<T> {
    fn from_wire(w: W) -> Result<Self, BindError> {
        Ok(Arc::new(T::from_wire(w)?))
    }
}

impl<T: IntoWire<W> + ?Sized, W> IntoWire<W> for Arc<T> {
    fn into_wire(&self) -> Result<W, BindError> {
        (**self).into_wire()
    }
}

// ───────────────────────────── options and lists ─────────────────────────────

impl<T: FromWire<W>, W> FromWire<Option<W>> for Option<T> {
    fn from_wire(w: Option<W>) -> Result<Self, BindError> {
        w.map(T::from_wire).transpose()
    }
}

impl<T: IntoWire<W>, W> IntoWire<Option<W>> for Option<T> {
    fn into_wire(&self) -> Result<Option<W>, BindError> {
        self.as_ref().map(IntoWire::into_wire).transpose()
    }
}

impl<T: FromWire<W>, W> FromWire<Vec<W>> for Vec<T> {
    fn from_wire(w: Vec<W>) -> Result<Self, BindError> {
        w.into_iter().map(T::from_wire).collect()
    }
}

impl<T: IntoWire<W>, W> IntoWire<Vec<W>> for Vec<T> {
    fn into_wire(&self) -> Result<Vec<W>, BindError> {
        self.iter().map(IntoWire::into_wire).collect()
    }
}

impl<T: IntoWire<W>, W> IntoWire<Vec<W>> for [T] {
    fn into_wire(&self) -> Result<Vec<W>, BindError> {
        self.iter().map(IntoWire::into_wire).collect()
    }
}

impl<T: FromWire<W>, W> FromWire<Vec<W>> for Box<[T]> {
    fn from_wire(w: Vec<W>) -> Result<Self, BindError> {
        w.into_iter().map(T::from_wire).collect()
    }
}

impl<T: FromWire<W>, W> FromWire<Vec<W>> for Arc<[T]> {
    fn from_wire(w: Vec<W>) -> Result<Self, BindError> {
        w.into_iter().map(T::from_wire).collect()
    }
}

impl<T: FromWire<W>, W> FromWire<Vec<W>> for std::collections::VecDeque<T> {
    fn from_wire(w: Vec<W>) -> Result<Self, BindError> {
        w.into_iter().map(T::from_wire).collect()
    }
}

impl<T: IntoWire<W>, W> IntoWire<Vec<W>> for std::collections::VecDeque<T> {
    fn into_wire(&self) -> Result<Vec<W>, BindError> {
        self.iter().map(IntoWire::into_wire).collect()
    }
}

impl<T: FromWire<W> + Ord, W> FromWire<Vec<W>> for std::collections::BTreeSet<T> {
    fn from_wire(w: Vec<W>) -> Result<Self, BindError> {
        w.into_iter().map(T::from_wire).collect()
    }
}

impl<T: IntoWire<W>, W> IntoWire<Vec<W>> for std::collections::BTreeSet<T> {
    fn into_wire(&self) -> Result<Vec<W>, BindError> {
        self.iter().map(IntoWire::into_wire).collect()
    }
}

impl<T: IntoWire<W>, W, const N: usize> IntoWire<Vec<W>> for [T; N] {
    fn into_wire(&self) -> Result<Vec<W>, BindError> {
        self.iter().map(IntoWire::into_wire).collect()
    }
}

impl<T: FromWire<W> + Copy + Default, W, const N: usize> FromWire<Vec<W>> for [T; N] {
    fn from_wire(w: Vec<W>) -> Result<Self, BindError> {
        if w.len() != N {
            return Err(BindError::Invalid("array length"));
        }
        let mut out = [T::default(); N];
        for (slot, x) in out.iter_mut().zip(w) {
            *slot = T::from_wire(x)?;
        }
        Ok(out)
    }
}

// maps: a list of pairs in the map's order (the `Tup*` struct of the pair is bound `tuple`)
impl<K, V, P> FromWire<Vec<P>> for indexmap::IndexMap<K, V>
where
    (K, V): FromWire<P>,
    K: std::hash::Hash + Eq,
{
    fn from_wire(w: Vec<P>) -> Result<Self, BindError> {
        w.into_iter().map(<(K, V)>::from_wire).collect()
    }
}

impl<K: Clone, V: Clone, P> IntoWire<Vec<P>> for indexmap::IndexMap<K, V>
where
    (K, V): IntoWire<P>,
{
    fn into_wire(&self) -> Result<Vec<P>, BindError> {
        self.iter().map(|(k, v)| (k.clone(), v.clone()).into_wire()).collect()
    }
}

impl<V, P> FromWire<Vec<P>> for JsRecord<V>
where
    (Box<str>, V): FromWire<P>,
{
    fn from_wire(w: Vec<P>) -> Result<Self, BindError> {
        let mut r = JsRecord::new();
        for p in w {
            let (k, v) = <(Box<str>, V)>::from_wire(p)?;
            r.insert(&k, v);
        }
        Ok(r)
    }
}

impl<V: Clone, P> IntoWire<Vec<P>> for JsRecord<V>
where
    (Box<str>, V): IntoWire<P>,
{
    fn into_wire(&self) -> Result<Vec<P>, BindError> {
        self.iter().map(|(k, v)| (Box::<str>::from(k), v.clone()).into_wire()).collect()
    }
}

// ───────────────────────────── primitives ─────────────────────────────

macro_rules! same {
    ($($t:ty),*) => {$(
        impl FromWire<$t> for $t {
            fn from_wire(w: $t) -> Result<Self, BindError> {
                Ok(w)
            }
        }
        impl IntoWire<$t> for $t {
            fn into_wire(&self) -> Result<$t, BindError> {
                Ok(*self)
            }
        }
    )*};
}
same!(bool, u8, u16, u32, i32, f64);

macro_rules! narrow {
    ($wire:ty => $($t:ty),*) => {$(
        impl FromWire<$wire> for $t {
            fn from_wire(w: $wire) -> Result<Self, BindError> {
                <$t>::try_from(w).map_err(|_| BindError::Range(stringify!($t)))
            }
        }
        impl IntoWire<$wire> for $t {
            fn into_wire(&self) -> Result<$wire, BindError> {
                <$wire>::try_from(*self).map_err(|_| BindError::Range(stringify!($t)))
            }
        }
    )*};
}
narrow!(i32 => i64, i8, i16);
narrow!(u32 => usize, u64);

impl FromWire<f64> for f32 {
    fn from_wire(w: f64) -> Result<Self, BindError> {
        // the wire carries what the shell has (a JS number); f32 is the native field's precision
        #[allow(clippy::cast_possible_truncation)]
        Ok(w as f32)
    }
}

impl IntoWire<f64> for f32 {
    fn into_wire(&self) -> Result<f64, BindError> {
        Ok(f64::from(*self))
    }
}

impl FromWire<u32> for char {
    fn from_wire(w: u32) -> Result<Self, BindError> {
        char::from_u32(w).ok_or(BindError::Invalid("char"))
    }
}

impl IntoWire<u32> for char {
    fn into_wire(&self) -> Result<u32, BindError> {
        Ok(u32::from(*self))
    }
}

impl FromWire<f64> for JsNumber {
    fn from_wire(w: f64) -> Result<Self, BindError> {
        Ok(JsNumber::Num(w))
    }
}

impl IntoWire<f64> for JsNumber {
    fn into_wire(&self) -> Result<f64, BindError> {
        Ok(self.as_f64().unwrap_or(f64::NAN))
    }
}

impl FromWire<f64> for NumKey {
    fn from_wire(w: f64) -> Result<Self, BindError> {
        Ok(NumKey::new(w))
    }
}

impl IntoWire<f64> for NumKey {
    fn into_wire(&self) -> Result<f64, BindError> {
        Ok(self.get())
    }
}

macro_rules! ids {
    ($($t:ident),*) => {$(
        impl FromWire<u32> for $t {
            fn from_wire(w: u32) -> Result<Self, BindError> {
                Ok($t(w))
            }
        }
        impl IntoWire<u32> for $t {
            fn into_wire(&self) -> Result<u32, BindError> {
                Ok(self.0)
            }
        }
    )*};
}
ids!(ParaId, TableId, DrawingId, HfPartId, MediaId, SectionBreakId, TxbxSrc, JobId, VersionId);

// ───────────────────────────── strings ─────────────────────────────

impl FromWire<Vec<u16>> for Utf16 {
    fn from_wire(w: Vec<u16>) -> Result<Self, BindError> {
        Ok(Utf16::from_units(w))
    }
}

impl IntoWire<Vec<u16>> for Utf16 {
    fn into_wire(&self) -> Result<Vec<u16>, BindError> {
        Ok(self.units().to_vec())
    }
}

impl FromWire<Vec<u16>> for String {
    fn from_wire(w: Vec<u16>) -> Result<Self, BindError> {
        Ok(String::from_utf16_lossy(&w))
    }
}

impl IntoWire<Vec<u16>> for String {
    fn into_wire(&self) -> Result<Vec<u16>, BindError> {
        Ok(self.encode_utf16().collect())
    }
}

impl IntoWire<Vec<u16>> for str {
    fn into_wire(&self) -> Result<Vec<u16>, BindError> {
        Ok(self.encode_utf16().collect())
    }
}

impl FromWire<Vec<u16>> for Box<str> {
    fn from_wire(w: Vec<u16>) -> Result<Self, BindError> {
        Ok(String::from_utf16_lossy(&w).into_boxed_str())
    }
}

impl FromWire<Vec<u16>> for Arc<str> {
    fn from_wire(w: Vec<u16>) -> Result<Self, BindError> {
        Ok(Arc::from(String::from_utf16_lossy(&w)))
    }
}

impl IntoWire<Vec<u16>> for std::borrow::Cow<'_, str> {
    fn into_wire(&self) -> Result<Vec<u16>, BindError> {
        Ok(self.encode_utf16().collect())
    }
}
