//! The conversions between the editor's wire types (`codec::generated`, from `schema/slides_edit.uos`) and uo-pptx's
//! edit types: the traits the generated `codec::bind` implements per bound type, the leaf impls it relies on, and the
//! three manual binds (`TransitionSpec`, `ShowSettingsSpec`, `ClipRel`: their Rust fields `type_`/`loop_` are not
//! schema field names).
//!
//! The traits have the shape of the Write session's (`crate::session::wire`), which is behind the `session` feature
//! (the Write editor module); the Slides editor module does not build it, so the few leaves it needs are here.
//!
//! Declared narrowings (record `uo-bindings.md` "Slides editor (B8 routing)"): `i64` travels as `i32` (out of range =
//! [`BindError::Range`]), `u64` as `f64` (a non-integer, negative or above 2^53 value = [`BindError::Range`]); `String`
//! arguments are built from UTF-16 with a lone surrogate replaced by U+FFFD, the `TextEncoder` rule of the TS.

use std::sync::Arc;

use uo_pptx::edit::doc::{ClipRel, ShowSettingsSpec};
use uo_pptx::edit::pkg::TransitionSpec;

use super::codec as w;

/// Why a wire value has no native form (or the other way).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    /// a number outside the native type's range
    Range(&'static str),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::Range(w) => write!(f, "out of range: {w}"),
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
    #[allow(clippy::wrong_self_convention, reason = "the value stays the editor's: the wire form is a copy")]
    fn into_wire(&self) -> Result<W, BindError>;
}

impl<T: IntoWire<W> + ?Sized, W> IntoWire<W> for &T {
    fn into_wire(&self) -> Result<W, BindError> {
        (**self).into_wire()
    }
}

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
same!(bool, f64, u32, i32);

impl FromWire<i32> for i64 {
    fn from_wire(w: i32) -> Result<Self, BindError> {
        Ok(i64::from(w))
    }
}

impl IntoWire<i32> for i64 {
    fn into_wire(&self) -> Result<i32, BindError> {
        i32::try_from(*self).map_err(|_| BindError::Range("i64 as i32"))
    }
}

/// The largest integer an `f64` holds exactly (2^53 − 1).
const MAX_SAFE: u64 = (1 << 53) - 1;

impl FromWire<f64> for u64 {
    fn from_wire(w: f64) -> Result<Self, BindError> {
        // the exact non-negative integers of the f64 range only, converted in two 32-bit halves (no float cast)
        if !uo_jsnum::conv::is_safe_integer(w) || w < 0.0 {
            return Err(BindError::Range("u64 as f64"));
        }
        let hi = (w / 4_294_967_296.0).floor();
        let lo = w - hi * 4_294_967_296.0;
        Ok((u64::from(uo_jsnum::conv::to_uint32(hi)) << 32) | u64::from(uo_jsnum::conv::to_uint32(lo)))
    }
}

impl IntoWire<f64> for u64 {
    fn into_wire(&self) -> Result<f64, BindError> {
        if *self > MAX_SAFE {
            return Err(BindError::Range("u64 as f64"));
        }
        let v = u32::try_from(*self >> 32).unwrap_or(0);
        let lo = u32::try_from(*self & 0xFFFF_FFFF).unwrap_or(0);
        Ok(f64::from(v) * 4_294_967_296.0 + f64::from(lo))
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

impl IntoWire<Vec<u16>> for Box<str> {
    fn into_wire(&self) -> Result<Vec<u16>, BindError> {
        Ok(self.encode_utf16().collect())
    }
}

impl FromWire<Vec<u8>> for Arc<[u8]> {
    fn from_wire(w: Vec<u8>) -> Result<Self, BindError> {
        Ok(Arc::from(w))
    }
}

impl IntoWire<Vec<u8>> for Arc<[u8]> {
    fn into_wire(&self) -> Result<Vec<u8>, BindError> {
        Ok(self.to_vec())
    }
}

// ───────────────────────────── the manual binds ─────────────────────────────

impl FromWire<w::TransitionSpec> for TransitionSpec {
    fn from_wire(v: w::TransitionSpec) -> Result<Self, BindError> {
        Ok(TransitionSpec {
            type_: FromWire::from_wire(v.r#type)?,
            dir: FromWire::from_wire(v.dir)?,
            speed: FromWire::from_wire(v.speed)?,
            advance_on_click: v.advance_on_click,
            advance_after_ms: v.advance_after_ms,
        })
    }
}

impl IntoWire<w::TransitionSpec> for TransitionSpec {
    fn into_wire(&self) -> Result<w::TransitionSpec, BindError> {
        let TransitionSpec { type_, dir, speed, advance_on_click, advance_after_ms } = self;
        Ok(w::TransitionSpec {
            r#type: IntoWire::into_wire(type_)?,
            dir: IntoWire::into_wire(dir)?,
            speed: IntoWire::into_wire(speed)?,
            advance_on_click: *advance_on_click,
            advance_after_ms: *advance_after_ms,
        })
    }
}

impl FromWire<w::ShowSettingsSpec> for ShowSettingsSpec {
    fn from_wire(v: w::ShowSettingsSpec) -> Result<Self, BindError> {
        Ok(ShowSettingsSpec { loop_: v.r#loop, use_timings: v.use_timings, show_animation: v.show_animation, kiosk: v.kiosk })
    }
}

impl IntoWire<w::ShowSettingsSpec> for ShowSettingsSpec {
    fn into_wire(&self) -> Result<w::ShowSettingsSpec, BindError> {
        let ShowSettingsSpec { loop_, use_timings, show_animation, kiosk } = self;
        Ok(w::ShowSettingsSpec { r#loop: *loop_, use_timings: *use_timings, show_animation: *show_animation, kiosk: *kiosk })
    }
}

impl FromWire<w::ClipRel> for ClipRel {
    fn from_wire(v: w::ClipRel) -> Result<Self, BindError> {
        Ok(ClipRel {
            id: FromWire::from_wire(v.id)?,
            type_: FromWire::from_wire(v.r#type)?,
            target: FromWire::from_wire(v.target)?,
            external: v.external,
            bytes: FromWire::from_wire(v.bytes)?,
            content_type: FromWire::from_wire(v.content_type)?,
        })
    }
}

impl IntoWire<w::ClipRel> for ClipRel {
    fn into_wire(&self) -> Result<w::ClipRel, BindError> {
        let ClipRel { id, type_, target, external, bytes, content_type } = self;
        Ok(w::ClipRel {
            id: IntoWire::into_wire(id)?,
            r#type: IntoWire::into_wire(type_)?,
            target: IntoWire::into_wire(target)?,
            external: *external,
            bytes: IntoWire::into_wire(bytes)?,
            content_type: IntoWire::into_wire(content_type)?,
        })
    }
}
