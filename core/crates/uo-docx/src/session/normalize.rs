//! The undo normalisation (architecture A3 "Restore", U4): what the TS JSON round trip of a
//! history step (`stringifyState` → `JSON.parse`) does to the model, applied once when a step is
//! restored, never when it is taken (the snapshot stays O(touched chunks)).
//!
//! | TS effect of `JSON.parse(JSON.stringify(state))` | here |
//! |---|---|
//! | keys holding `undefined` vanish | the cleared mask is dropped, and the key leaves the insertion order |
//! | `NaN`, `±Infinity` become `null` | [`JsNumber::Num`] of a non-finite value becomes [`JsNumber::Null`] |
//! | `-0` becomes `0` | `Num(-0.0)` becomes `Num(0.0)` (also inside [`JsonValue`]) |
//! | symbol keys vanish (`RPR_REF`, `RPR_EDITED`) | [`Run::rpr_link`] is dropped |
//! | every object is a new object | fresh [`ParaId`], [`TableId`], [`DrawingId`] |
//! | `Uint8Array`s go through the image pool (same object back) | media references are kept |
//! | the textbox's `w:sdt`/`w:customXml` wrappers are lost (the `WeakMap` misses) | [`TxbxSrc`] is kept: **fixed by name** (U4c) |
//! | custGeom numbers that were NaN read as `null` (0 in arithmetic) | `f64` NaN/±∞ → `0.0` (F13) |
//!
//! [`JsNormalize`] is implemented for every [`crate::js_object!`] type by the macro (feature
//! `session`) and here for the leaves and containers. `js_needs` lets a shared value skip its
//! copy-on-write when nothing would change (interned props stay shared).

use std::sync::Arc;

use uo_drawingml::{CustCmd, CustPath};

use crate::ids::{DrawingId, HfPartId, IdAlloc, MediaId, ParaId, SourceKey, TableId, TxbxSrc};
use crate::js::{FieldMask, JsNumber, JsRecord, JsonValue, KeyOrder, Nullable};
use crate::model::color::DrawingColor;
use crate::model::media::MediaRef;
use crate::model::para::SdtKey;
use crate::model::run::{RprLink, TextboxView};
use crate::model::sources::{SrcText, WrapId};
use crate::model::story::{Story, TableAnchor};
use crate::model::table::CellView;
use crate::pvec::PVec;
use crate::text::Utf16;

/// What the normalisation needs besides the value: the document's id allocator (the JSON copies
/// are new objects, G22).
pub struct NormCx<'a> {
    pub ids: &'a mut IdAlloc,
}

/// One value of the model under the undo normalisation (see the module comment).
pub trait JsNormalize {
    /// Whether [`JsNormalize::js_normalize`] would change anything.
    fn js_needs(&self) -> bool;
    /// Applies the normalisation in place.
    fn js_normalize(&mut self, cx: &mut NormCx<'_>);
}

/// Drops the explicit-`undefined` keys of a declared object: the bits, and their places in the
/// insertion order (a key assigned later goes last, as on a fresh JSON object).
pub fn drop_cleared(cleared: &mut FieldMask, keys: &mut KeyOrder) {
    if cleared.is_empty() {
        return;
    }
    let bits = cleared.bits();
    for i in 0..64u8 {
        if bits & (1u64 << i) != 0 {
            keys.forget(i);
        }
    }
    *cleared = FieldMask::default();
}

/// Normalises a whole value with a fresh context (helper for callers holding the allocator).
pub fn normalize<T: JsNormalize + ?Sized>(v: &mut T, ids: &mut IdAlloc) {
    v.js_normalize(&mut NormCx { ids });
}

/// A number after `JSON.parse(JSON.stringify(x))`.
#[must_use]
pub fn json_number(x: f64) -> JsNumber {
    if !x.is_finite() {
        JsNumber::Null
    } else if x == 0.0 {
        JsNumber::Num(0.0)
    } else {
        JsNumber::Num(x)
    }
}

fn num_needs(x: f64) -> bool {
    !x.is_finite() || (x == 0.0 && x.is_sign_negative())
}

impl JsNormalize for JsNumber {
    fn js_needs(&self) -> bool {
        matches!(self, JsNumber::Num(x) if num_needs(*x))
    }
    fn js_normalize(&mut self, _: &mut NormCx<'_>) {
        if let JsNumber::Num(x) = *self {
            *self = json_number(x);
        }
    }
}

/// Plain `f64` fields (custGeom): NaN/±∞ → 0 (F13: the TS `null` reads as 0 in every use).
impl JsNormalize for f64 {
    fn js_needs(&self) -> bool {
        num_needs(*self)
    }
    fn js_normalize(&mut self, _: &mut NormCx<'_>) {
        if !self.is_finite() || *self == 0.0 {
            *self = 0.0;
        }
    }
}

macro_rules! unchanged {
    ($($t:ty),* $(,)?) => {
        $(
            impl JsNormalize for $t {
                fn js_needs(&self) -> bool {
                    false
                }
                fn js_normalize(&mut self, _: &mut NormCx<'_>) {}
            }
        )*
    };
}

// strings (no lone surrogate change: `JSON.stringify` escapes them and `JSON.parse` restores
// them), booleans, values that survive (source keys, media through the pool, the textbox's source
// groups (U4c), header/footer parts (never in the JSON)), the views (the session re-aliases)
unchanged!(
    bool,
    u32,
    str,
    Utf16,
    SourceKey,
    MediaId,
    MediaRef,
    TxbxSrc,
    HfPartId,
    SrcText,
    SdtKey,
    WrapId,
    DrawingColor,
    CellView,
    TextboxView,
    // the `type: 'table'` tag; the source store's sets (never in an undo step: the store is not
    // in `stringifyState`)
    crate::model::table::TableTag,
    crate::model::sources::NumSet,
);

/// The textbox's views become copies (the JSON round trip; the canon then writes both).
impl JsNormalize for crate::model::run::ViewsCopied {
    fn js_needs(&self) -> bool {
        !self.0
    }
    fn js_normalize(&mut self, _: &mut NormCx<'_>) {
        self.0 = true;
    }
}

/// Wave 7 (B): an editing view is session state, not JSON: the round trip drops it (the session
/// rebuilds it from the blocks, `session/ops/hf_flat.rs`).
impl<T> JsNormalize for crate::model::section::EditSlot<T> {
    fn js_needs(&self) -> bool {
        self.0.is_some()
    }
    fn js_normalize(&mut self, _: &mut NormCx<'_>) {
        self.0 = None;
    }
}

impl JsNormalize for ParaId {
    fn js_needs(&self) -> bool {
        true
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        *self = cx.ids.para();
    }
}

impl JsNormalize for TableId {
    fn js_needs(&self) -> bool {
        true
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        *self = cx.ids.table();
    }
}

impl JsNormalize for DrawingId {
    fn js_needs(&self) -> bool {
        true
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        *self = cx.ids.drawing();
    }
}

/// The symbol keys vanish: the link is emptied (an empty link is no link: `syncRunRPr` reads
/// `props[RPR_REF]` and `props[RPR_EDITED]` as falsy).
impl JsNormalize for RprLink {
    fn js_needs(&self) -> bool {
        self.rpr.is_some() || self.edited
    }
    fn js_normalize(&mut self, _: &mut NormCx<'_>) {
        self.rpr = None;
        self.edited = false;
    }
}

impl JsNormalize for JsonValue {
    fn js_needs(&self) -> bool {
        match self {
            JsonValue::Num(x) => num_needs(*x),
            JsonValue::Array(a) => a.iter().any(JsNormalize::js_needs),
            JsonValue::Object(o) => o.values().any(JsNormalize::js_needs),
            JsonValue::Null | JsonValue::Bool(_) | JsonValue::Str(_) => false,
        }
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        match self {
            JsonValue::Num(x) => {
                if let JsNumber::Num(y) = json_number(*x) {
                    *x = y;
                } else {
                    *self = JsonValue::Null;
                }
            }
            JsonValue::Array(a) => a.js_normalize(cx),
            JsonValue::Object(o) => o.js_normalize(cx),
            JsonValue::Null | JsonValue::Bool(_) | JsonValue::Str(_) => {}
        }
    }
}

impl JsNormalize for CustPath {
    fn js_needs(&self) -> bool {
        num_needs(self.w) || num_needs(self.h) || self.cmds.iter().any(cmd_needs)
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        self.w.js_normalize(cx);
        self.h.js_normalize(cx);
        for c in &mut self.cmds {
            match c {
                CustCmd::MoveTo { x, y } | CustCmd::LineTo { x, y } => {
                    x.js_normalize(cx);
                    y.js_normalize(cx);
                }
                CustCmd::CubicTo(pts) | CustCmd::QuadTo(pts) => {
                    for [x, y] in pts.iter_mut() {
                        x.js_normalize(cx);
                        y.js_normalize(cx);
                    }
                }
                CustCmd::ArcTo { w_r, h_r, st, sw } => {
                    for v in [w_r, h_r, st, sw] {
                        v.js_normalize(cx);
                    }
                }
                CustCmd::Close => {}
            }
        }
    }
}

fn cmd_needs(c: &CustCmd) -> bool {
    match c {
        CustCmd::MoveTo { x, y } | CustCmd::LineTo { x, y } => num_needs(*x) || num_needs(*y),
        CustCmd::CubicTo(pts) | CustCmd::QuadTo(pts) => pts.iter().any(|[x, y]| num_needs(*x) || num_needs(*y)),
        CustCmd::ArcTo { w_r, h_r, st, sw } => [*w_r, *h_r, *st, *sw].into_iter().any(num_needs),
        CustCmd::Close => false,
    }
}

impl<T: JsNormalize> JsNormalize for Option<T> {
    fn js_needs(&self) -> bool {
        self.as_ref().is_some_and(JsNormalize::js_needs)
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        if let Some(v) = self {
            v.js_normalize(cx);
        }
    }
}

impl<T: JsNormalize + ?Sized> JsNormalize for Box<T> {
    fn js_needs(&self) -> bool {
        (**self).js_needs()
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        (**self).js_normalize(cx);
    }
}

/// Copy-on-write only when something changes (interned or snapshot-shared values stay shared).
impl<T: JsNormalize + Clone> JsNormalize for Arc<T> {
    fn js_needs(&self) -> bool {
        (**self).js_needs()
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        if (**self).js_needs() {
            Arc::make_mut(self).js_normalize(cx);
        }
    }
}

impl<T: JsNormalize> JsNormalize for Vec<T> {
    fn js_needs(&self) -> bool {
        self.iter().any(JsNormalize::js_needs)
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        for v in self {
            v.js_normalize(cx);
        }
    }
}

impl<A: JsNormalize, B: JsNormalize> JsNormalize for (A, B) {
    fn js_needs(&self) -> bool {
        self.0.js_needs() || self.1.js_needs()
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        self.0.js_normalize(cx);
        self.1.js_normalize(cx);
    }
}

impl<T: JsNormalize> JsNormalize for Nullable<T> {
    fn js_needs(&self) -> bool {
        matches!(self, Nullable::Val(v) if v.js_needs())
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        if let Nullable::Val(v) = self {
            v.js_normalize(cx);
        }
    }
}

impl<T: JsNormalize> JsNormalize for JsRecord<T> {
    fn js_needs(&self) -> bool {
        self.values().any(JsNormalize::js_needs)
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        for v in self.values_mut() {
            v.js_normalize(cx);
        }
    }
}

impl<T: JsNormalize + Clone> JsNormalize for PVec<T> {
    fn js_needs(&self) -> bool {
        self.iter().any(JsNormalize::js_needs)
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        for i in 0..self.len() {
            if self.get(i).is_some_and(JsNormalize::js_needs)
                && let Some(v) = self.make_mut(i)
            {
                v.js_normalize(cx);
            }
        }
    }
}

impl JsNormalize for TableAnchor {
    fn js_needs(&self) -> bool {
        self.table.js_needs()
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        self.table.js_normalize(cx);
    }
}

impl JsNormalize for Story {
    fn js_needs(&self) -> bool {
        !self.paras.is_empty() || !self.tables.is_empty()
    }
    fn js_normalize(&mut self, cx: &mut NormCx<'_>) {
        self.paras.js_normalize(cx);
        self.tables.js_normalize(cx);
    }
}
