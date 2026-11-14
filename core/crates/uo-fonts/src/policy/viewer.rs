//! The `viewer` policy: `ViewerFonts.provider()` of `write/packages/viewer/src/fonts.ts:90-137`,
//! over what the host pushed into the [`FontRegistry`].
//!
//! The viewer, the editor and the PDF measure and paint with the **file** of the font: a family
//! that exists in the browser only by name, or whose licence does not let the PDF embed it, is
//! answered by its metric-compatible substitute. The order, from the TS:
//!
//! 1. the hosted asset files for the family (`push_asset`), in the variant order below;
//! 2. the resolver's answer for the family (`push_family`): none gives `None` and notes the
//!    variant as missing; `source == Default` gives `None` (the engine follows its own chain);
//!    a variant the family lacks is noted as missing;
//! 3. for each variant in the order, the pushed file; when the licence does not let it embed, the
//!    substitute family's file of that variant (else its regular one), noting the substitute as
//!    missing when it is not there; a file that does not parse is skipped.
//!
//! The variant order is the wanted one, then bold (or regular), italic (or regular) and regular,
//! without repeats. The answer's key is `family|variant`, `synth_bold`/`synth_italic` say the
//! variant found lacks what was asked, and `stand_in` is `source == Substitute`.
//!
//! **Identity**: `provider()` of the TS keeps a cache per provider object, keyed `family|bi` with
//! the family as asked (case-sensitive), and the viewer makes one provider per layout call. A
//! [`ViewerProvider`] is that object: one per layout call, repeated asks answer the same
//! [`FontAnswer`] (the `null`s included), different spellings or variants are different answers
//! even over the same file.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use uo_jsnum::text::to_lowercase;

use crate::catalog::metric_substitute;
use crate::registry::{AnswerParts, FontAnswer, FontProvider, FontRegistry, FontSource, Pushed, PushedFace};
use crate::variant::{Variant, is_bold_variant, is_italic_variant, variant_of};

/// Distinct requests one provider remembers (M5); past it answers are computed again.
pub const MAX_VIEWER_REQUESTS: usize = 16_384;

type Key = (String, bool, bool);

/// The provider of one layout call.
pub struct ViewerProvider {
    registry: Arc<FontRegistry>,
    cache: Mutex<HashMap<Key, Option<Arc<FontAnswer>>>>,
}

impl ViewerProvider {
    pub fn new(registry: Arc<FontRegistry>) -> ViewerProvider {
        ViewerProvider { registry, cache: Mutex::new(HashMap::new()) }
    }

    /// Requests remembered (diagnostics, M5 tests).
    pub fn remembered(&self) -> usize {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner).len()
    }
}

impl FontProvider for ViewerProvider {
    fn budget_refusals(&self) -> u64 {
        self.registry.ledger().refused()
    }

    /// WordCraft study (fields, C5): the full names (name ID 4) of the faces the host pushed (Local Font Access, host
    /// fonts, the page's files): [`FontRegistry::full_name_face`].
    fn full_name_face(&self, name: &str, ui_lcid: u16) -> Option<(String, bool, bool)> {
        self.registry.full_name_face(name, ui_lcid)
    }

    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
        let key: Key = (family.to_owned(), bold, italic);
        if let Some(hit) = self.cache.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return hit.clone();
        }
        let out = resolve(&self.registry, family, bold, italic);
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(first) = cache.get(&key) {
            return first.clone();
        }
        if cache.len() < MAX_VIEWER_REQUESTS {
            cache.insert(key, out.clone());
        }
        out
    }
}

fn order_of(want: Variant, bold: bool, italic: bool) -> Vec<Variant> {
    let mut order = Vec::with_capacity(4);
    for v in [
        want,
        if bold { Variant::Bold } else { Variant::Regular },
        if italic { Variant::Italic } else { Variant::Regular },
        Variant::Regular,
    ] {
        if !order.contains(&v) {
            order.push(v);
        }
    }
    order
}

fn index(v: Variant) -> usize {
    match v {
        Variant::Regular => 0,
        Variant::Bold => 1,
        Variant::Italic => 2,
        Variant::BoldItalic => 3,
    }
}

fn answer(family: &str, pushed: &PushedFace, bold: bool, italic: bool, v: Variant, stand_in: bool) -> Arc<FontAnswer> {
    FontAnswer::new(AnswerParts {
        key: format!("{family}|{}", v.name()),
        family: family.to_owned(),
        face: pushed.face.clone(),
        bold,
        italic,
        synth_bold: bold && !is_bold_variant(v),
        synth_italic: italic && !is_italic_variant(v),
        stand_in,
        advances: pushed.advances.clone(),
        ticket: Some(pushed.ticket.clone()),
    })
}

fn resolve(reg: &FontRegistry, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
    let want = variant_of(bold, italic);
    let order = order_of(want, bold, italic);
    if let Some(assets) = reg.assets(family) {
        for v in &order {
            if let Some(p) = assets.faces[index(*v)].as_ref().and_then(|p| reg.pushed_face(p)) {
                return Some(answer(family, &p, bold, italic, *v, false));
            }
        }
    }
    let Some(r) = reg.family(family) else {
        reg.note_missing(family, want);
        return None;
    };
    if r.source == FontSource::Default {
        return None;
    }
    if r.faces[index(want)].is_none() {
        reg.note_missing(family, want);
    }
    for v in &order {
        let Some(found) = r.faces[index(*v)].as_ref() else { continue };
        let (chosen, fam): (Arc<Pushed>, &str) = if r.embeddable {
            (Arc::clone(found), family)
        } else {
            let sub = metric_substitute(&to_lowercase(family));
            let rs = sub.and_then(|s| reg.family(s));
            let b2 = rs.and_then(|rs| rs.faces[index(*v)].clone().or_else(|| rs.faces[index(Variant::Regular)].clone()));
            match (b2, sub) {
                (Some(b), Some(s)) => (b, s),
                (_, sub) => {
                    if let Some(s) = sub {
                        reg.note_missing(s, *v);
                    }
                    continue;
                }
            }
        };
        if let Some(face) = reg.pushed_face(&chosen) {
            return Some(answer(fam, &face, bold, italic, *v, r.source == FontSource::Substitute));
        }
    }
    None
}
