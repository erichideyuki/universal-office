//! Dense ids of the per-layout resource tables, in first-use order (the codec sends resource
//! deltas as "entries from index n", uo-render §3.2). Each id is `index + 1` in a `NonZeroU32`,
//! so `Option<Id>` costs no more than the id (the item size budget, RA1.3).

use std::num::NonZeroU32;

macro_rules! dense_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
        pub struct $name(NonZeroU32);

        impl $name {
            /// The id of table entry `index`; `None` past `u32::MAX - 1` entries.
            #[must_use]
            pub fn from_index(index: usize) -> Option<$name> {
                u32::try_from(index).ok().and_then(|i| i.checked_add(1)).and_then(NonZeroU32::new).map($name)
            }

            /// The table index of this id.
            #[must_use]
            pub fn index(self) -> usize {
                // u32 -> usize never truncates on the 32- and 64-bit targets the core builds for
                (self.0.get() - 1) as usize
            }

            /// The raw value (`index + 1`) as the codec writes it.
            #[must_use]
            pub fn raw(self) -> u32 {
                self.0.get()
            }

            /// The id of a raw value read by a decoder; `None` for 0.
            #[must_use]
            pub fn from_raw(raw: u32) -> Option<$name> {
                NonZeroU32::new(raw).map($name)
            }
        }
    };
}

dense_id!(
    /// An interned UTF-16 string of the layout ([`crate::SymbolTable`]): colours as the document
    /// wrote them, preset and dash names, extensions.
    Sym
);
dense_id!(
    /// A face of the layout: the identity of the font provider's answer (layout K2a), not the
    /// bytes (two answers with the same file are two faces, as two TS `FontFace` objects).
    FaceId
);
dense_id!(
    /// An image of the layout: the identity of the TS `InlineImage` object (RQ3: one per model
    /// drawing image, one per derived creation, one per pass for a page fill); the bytes are
    /// shared by content underneath.
    ImageId
);
dense_id!(
    /// A resolved w14 text-effect set (`ResolvedTextFx`), one per TS object.
    TextFxId
);
dense_id!(
    /// A resolved page fill (`PageFillPaint`).
    PageFillId
);
dense_id!(
    /// A custom geometry (`a:custGeom` paths) shared by the shapes that use it.
    CustId
);
