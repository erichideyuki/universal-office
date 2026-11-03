//! Headers and footers (frontier F7c): `layoutHeaderFooter` per page and slot (K13: the slot
//! names the ref, one part may serve several slots), header/footer frames and floats
//! (engine.ts:1100–1200, 1498–1510).

use crate::output::{HfSlot, HfWhich};

/// Which header/footer of a page: the slot used and the story laid out for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HfUse {
    pub which: HfWhich,
    pub slot: HfSlot,
    /// index into `LDoc::stories`
    pub story: u32,
}
