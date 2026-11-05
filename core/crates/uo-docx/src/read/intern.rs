//! The props interning of one read (record `uo-docx.md`, "Model size — P1 done"): equal props share
//! one `Arc`. Most runs of a paragraph, and of a document, have the same `RunProps`; most paragraphs
//! the same `ParaProps`; the TS stores each as its own object, and so did the reader, at 480 and
//! 392 bytes a struct.
//!
//! - **Equality is exact** ([`crate::js::JsValue::exact`]): numbers by their bits, the cleared mask
//!   and the recorded key order included, so two values the TS (or the digest, or the writer's
//!   signatures in either mode) could tell apart are never merged. A value whose type declares no
//!   exact identity, or whose key passes [`MAX_KEY_BYTES`], is not interned.
//! - **Sharing is not identity** (A1.3): nothing reads an interned pointer as TS identity; an edit
//!   goes through `Arc::make_mut`, which copies a shared value, as it would after an undo snapshot.
//!   The reader's own late writes (`run_sym`'s font, a note reference's `vertAlign`, the TOC
//!   instruction, the block content control) copy too and re-intern the result.
//! - **Per read**: the table lives in the read's [`super::ctx::ReadCtx`] and is dropped with it (no
//!   process state); its memory is bounded ([`MAX_ENTRIES`], [`MAX_TABLE_BYTES`]): past either bound
//!   it stops taking new values and keeps answering the ones it has.
//! - With sources, `RunProps` and `ParaProps` are not interned: each gets its own `src` right after
//!   its paragraph is read (`SourceStore::add_para`, `capture_paragraph`), so no two are equal.

use std::collections::HashMap;
use std::sync::Arc;

use crate::js::{ExactKey, JsValue};
use crate::model::props::{PPr, RPr, TblPr, TcPr, TrPr};
use crate::model::{ParaProps, RunProps};

/// The longest exact key interned (a props value with a longer one — long link targets, a large
/// tab list — keeps its own `Arc`).
pub const MAX_KEY_BYTES: usize = 4096;
/// Distinct values kept per read, all kinds together.
pub const MAX_ENTRIES: usize = 16_384;
/// Bytes the keys of a read may hold, all kinds together (with [`ENTRY_OVERHEAD`] per entry).
pub const MAX_TABLE_BYTES: usize = 1 << 20;
/// The bookkeeping counted per entry on top of its key: the boxed key's pointer and length, the
/// `Arc` and the hash table's slot and control byte.
const ENTRY_OVERHEAD: usize = 48;

/// One table: exact key → the shared value.
type Table<T> = HashMap<Box<[u8]>, Arc<T>>;

/// The props types the reader interns.
pub trait Interned: JsValue + Sized {
    /// Whether the read with sources gives every value its own `src` (then it is not interned).
    const GETS_SRC: bool;
    #[doc(hidden)]
    fn table(t: &mut Tables) -> &mut Table<Self>;
}

/// The interning tables of one read (see the module comment).
#[derive(Debug)]
pub struct Interner {
    key: ExactKey,
    entries: usize,
    bytes: usize,
    tables: Tables,
}

/// One table per kind.
#[doc(hidden)]
#[derive(Debug, Default)]
pub struct Tables {
    run_props: Table<RunProps>,
    rpr: Table<RPr>,
    para_props: Table<ParaProps>,
    ppr: Table<PPr>,
    tc_pr: Table<TcPr>,
    tr_pr: Table<TrPr>,
    tbl_pr: Table<TblPr>,
}

impl Default for Interner {
    fn default() -> Self {
        Interner { key: ExactKey::new(MAX_KEY_BYTES), entries: 0, bytes: 0, tables: Tables::default() }
    }
}

macro_rules! interned {
    ($($t:ty => $field:ident, $src:expr;)*) => {$(
        impl Interned for $t {
            const GETS_SRC: bool = $src;
            fn table(t: &mut Tables) -> &mut Table<Self> {
                &mut t.$field
            }
        }
    )*};
}

interned! {
    RunProps => run_props, true;
    RPr => rpr, false;
    ParaProps => para_props, true;
    PPr => ppr, false;
    TcPr => tc_pr, false;
    TrPr => tr_pr, false;
    TblPr => tbl_pr, false;
}

impl Interner {
    /// The shared `Arc` of a value equal to `v`, or a new one (kept for the next equal value while
    /// the table has room).
    #[inline(never)]
    pub fn intern<T: Interned>(&mut self, v: T) -> Arc<T> {
        self.key.clear();
        if !v.exact(&mut self.key) {
            return Arc::new(v);
        }
        if let Some(shared) = T::table(&mut self.tables).get(self.key.bytes()) {
            return Arc::clone(shared);
        }
        let a = Arc::new(v);
        self.keep(&a);
        a
    }

    /// Replaces `a` with the shared `Arc` of an equal value, or keeps it for the next equal value.
    #[inline(never)]
    pub fn intern_shared<T: Interned>(&mut self, a: &mut Arc<T>) {
        self.key.clear();
        if !(**a).exact(&mut self.key) {
            return;
        }
        if let Some(shared) = T::table(&mut self.tables).get(self.key.bytes()) {
            *a = Arc::clone(shared);
            return;
        }
        self.keep(a);
    }

    /// Stores `a` under the key just written, within the bounds.
    fn keep<T: Interned>(&mut self, a: &Arc<T>) {
        let cost = self.key.bytes().len() + ENTRY_OVERHEAD;
        if self.entries >= MAX_ENTRIES || self.bytes + cost > MAX_TABLE_BYTES {
            return;
        }
        self.entries += 1;
        self.bytes += cost;
        let key: Box<[u8]> = self.key.bytes().into();
        T::table(&mut self.tables).insert(key, Arc::clone(a));
    }

    /// Distinct values kept, all kinds together.
    #[must_use]
    pub fn entries(&self) -> usize {
        self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::{JsNumber, JsObject};

    fn sz(x: f64) -> RunProps {
        RunProps { sz_half_pt: Some(JsNumber::Num(x)), ..RunProps::default() }
    }

    #[test]
    fn equal_values_share_and_distinguishable_ones_do_not() {
        let mut i = Interner::default();
        let a = i.intern(sz(24.0));
        let b = i.intern(sz(24.0));
        assert!(Arc::ptr_eq(&a, &b));
        // `-0` and `0`, NaN payloads, `null`: equal by `PartialEq` or not, never merged
        let z = i.intern(sz(0.0));
        let nz = i.intern(sz(-0.0));
        assert!(!Arc::ptr_eq(&z, &nz) && nz.sz_half_pt.and_then(JsNumber::as_f64).is_some_and(f64::is_sign_negative));
        let nan = i.intern(sz(f64::NAN));
        let nan2 = i.intern(sz(f64::from_bits(f64::NAN.to_bits() | 1)));
        assert!(Arc::ptr_eq(&nan, &i.intern(sz(f64::NAN))) && !Arc::ptr_eq(&nan, &nan2));
        let null = i.intern(RunProps { sz_half_pt: Some(JsNumber::Null), ..RunProps::default() });
        assert!(!Arc::ptr_eq(&null, &nan) && !Arc::ptr_eq(&null, &z));
        // the cleared mask: an absent key and a key holding `undefined`
        let mut cleared = RunProps::default();
        cleared.mark_undefined("color");
        let plain = i.intern(RunProps::default());
        let cleared = i.intern(cleared);
        assert!(!Arc::ptr_eq(&plain, &cleared));
        // the key order
        let mut ab = RunProps { bold: Some(true), italic: Some(true), ..RunProps::default() };
        ab.mark("italic");
        ab.mark("bold");
        let ba = RunProps { bold: Some(true), italic: Some(true), ..RunProps::default() };
        let (ab, ba) = (i.intern(ab), i.intern(ba));
        assert!(!Arc::ptr_eq(&ab, &ba));
        assert_eq!(i.entries(), 10);
        let mut shared = Arc::new(sz(24.0));
        i.intern_shared(&mut shared);
        assert!(Arc::ptr_eq(&shared, &a));
    }

    #[test]
    fn long_values_and_full_tables_keep_their_own_arc() {
        let mut i = Interner::default();
        let long = || RunProps { link: Some("x".repeat(MAX_KEY_BYTES).into()), ..RunProps::default() };
        assert!(!Arc::ptr_eq(&i.intern(long()), &i.intern(long())));
        assert_eq!(i.entries(), 0);
        for n in 0..MAX_ENTRIES + 10 {
            i.intern(sz(f64::from(u32::try_from(n).unwrap_or(u32::MAX))));
        }
        assert!(i.bytes <= MAX_TABLE_BYTES && i.entries <= MAX_ENTRIES && i.entries > 1000);
        let past = sz(1e9);
        assert!(!Arc::ptr_eq(&i.intern(past.clone()), &i.intern(past)));
        assert!(Arc::ptr_eq(&i.intern(sz(0.0)), &i.intern(sz(0.0))));
    }
}
