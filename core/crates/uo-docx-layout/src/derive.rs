//! Building the derived document of a call (LA2, frontier F7a): `liftFrames` (frames.ts:87) in
//! **every** story with its own `stops` rule (K3: the TS lifts in every non-cell `layoutStatic`
//! too — headers/footers and textbox contents), `indexChains` (chain.ts), `sliceParagraphFrom`
//! (chain.ts:57: slices are `RunSrc::Slice`, no text copied), `joinDeleted` and the revision-view
//! copies (revisions.ts:124–157: shallow, model runs keep their `RunRef`, K1), the revision author
//! order. Model paragraphs are `ParaSrc::Model`, every TS copy `ParaSrc::Copy`.

use crate::error::{Frontier, LResult, unported};
use crate::ldoc::LDoc;
use crate::model::DocView;
use crate::options::LayoutOptions;

/// The derived document of `view` for one call — F7a.
pub fn build<'a>(_view: DocView<'a>, _opts: &LayoutOptions) -> LResult<LDoc<'a>> {
    unported(Frontier::F7a, "derive::build")
}
