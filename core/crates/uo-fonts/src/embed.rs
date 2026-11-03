//! The embedding plan: which faces go into a document, with which bytes, and why the others do
//! not. The decision is one for every app; the format crates only write it. Two phases: the
//! caller asks [`embedding_requests`] which faces the plan will need, fetches exactly those, and
//! gives the answers to [`plan_font_embedding`].

use std::borrow::Cow;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::cmap::glyph_ids_for;
use crate::error::Result;
use crate::info::{FaceInfo, face_info, has_true_type_outlines, is_embeddable};
use crate::js::js_to_lowercase;
use crate::limits::FontLimits;
use crate::subset::subset_true_type;
use crate::variant::{VARIANTS, Variant};

/// What a program embeds and what it leaves out, resolved by the caller from its own rules.
#[derive(Debug, Clone, Copy)]
pub struct EmbeddingPolicy<'a> {
    /// embed fonts at all
    pub embed: bool,
    /// only the characters used
    pub subset: bool,
    /// leave out the common system fonts (the TS `app == slides || settings.skipCommonFonts`,
    /// resolved by the caller)
    pub skip_common_fonts: bool,
    /// the common fonts, compared lower-cased
    pub common_fonts: &'a [&'a str],
    /// true: the four variants of the family (PowerPoint); false: the used ones (Word)
    pub all_variants: bool,
}

/// The settings a user picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontEmbeddingSettings {
    pub embed: bool,
    pub subset: bool,
    pub skip_common_fonts: bool,
}

/// The fonts Word does not embed (measured).
pub const WORD_COMMON_FONTS: [&str; 3] = ["Arial", "Times New Roman", "Courier New"];

/// The fonts PowerPoint does not embed (measured).
pub const POWERPOINT_COMMON_FONTS: [&str; 5] = ["Arial", "Times New Roman", "Courier New", "Calibri", "Aptos"];

/// Word starts with embedding off, whole fonts, common fonts skipped.
pub const WORD_DEFAULT_EMBEDDING: FontEmbeddingSettings = FontEmbeddingSettings { embed: false, subset: false, skip_common_fonts: true };

/// PowerPoint starts with embedding off, subsets, common fonts skipped.
pub const POWERPOINT_DEFAULT_EMBEDDING: FontEmbeddingSettings =
    FontEmbeddingSettings { embed: false, subset: true, skip_common_fonts: true };

/// What the document uses of a family: the variants and the text written in it.
#[derive(Debug, Clone, Copy)]
pub struct FontUsage<'a> {
    pub family: &'a str,
    pub variants: &'a [Variant],
    pub text: &'a str,
}

/// A face the caller must fetch: the family (first spelling) and the variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceRequest {
    pub family: String,
    pub variant: Variant,
}

/// A face ready to be wrapped by the format: the bytes (shared with the source, or the subset).
#[derive(Debug, Clone)]
pub struct FaceToEmbed {
    pub family: String,
    pub variant: Variant,
    pub bytes: Arc<[u8]>,
    pub subsetted: bool,
    pub info: FaceInfo,
}

/// Why a family or face was left out, in the order the TS checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    Common,
    License,
    Unavailable,
    AlreadyEmbedded,
}

#[derive(Debug, Clone, Default)]
pub struct EmbeddingPlan {
    pub faces: Vec<FaceToEmbed>,
    pub skipped: Vec<(String, SkipReason)>,
}

/// What the walk over the usage decides for one family, in the order of the TS loop.
enum Step {
    /// left out before any face is fetched
    Skip(String, SkipReason),
    /// the faces to fetch: the wanted variants, and the code points the family is used for
    Family { family: String, variants: Vec<Variant>, chars: FxHashSet<u32> },
}

/// The usage grouped by lower-cased family (insertion order, the first spelling kept, the union
/// of the variants and of the code points of the text), then the skip rules in the TS order:
/// already embedded, then common fonts. Families without a name are not used.
fn walk(usage: &[FontUsage<'_>], policy: &EmbeddingPolicy<'_>, already_embedded: &FxHashSet<String>) -> Vec<Step> {
    struct Used {
        family: String,
        variants: [bool; 4],
        chars: FxHashSet<u32>,
    }
    let mut order: Vec<String> = Vec::new();
    let mut by_family: FxHashMap<String, Used> = FxHashMap::default();
    for u in usage {
        if u.family.is_empty() {
            continue;
        }
        let key = js_to_lowercase(u.family);
        let entry = by_family.entry(key.clone()).or_insert_with(|| {
            order.push(key);
            Used { family: u.family.to_owned(), variants: [false; 4], chars: FxHashSet::default() }
        });
        for v in u.variants {
            if let Some(slot) = entry.variants.get_mut(variant_index(*v)) {
                *slot = true;
            }
        }
        entry.chars.extend(u.text.chars().map(u32::from));
    }

    let common: FxHashSet<String> = policy.common_fonts.iter().map(|f| js_to_lowercase(f)).collect();
    let mut steps = Vec::new();
    for key in order {
        let Some(used) = by_family.remove(&key) else { continue };
        if already_embedded.contains(&key) {
            steps.push(Step::Skip(used.family, SkipReason::AlreadyEmbedded));
            continue;
        }
        if policy.skip_common_fonts && common.contains(&key) {
            steps.push(Step::Skip(used.family, SkipReason::Common));
            continue;
        }
        let variants: Vec<Variant> = VARIANTS
            .iter()
            .copied()
            .filter(|v| policy.all_variants || used.variants.get(variant_index(*v)).copied().unwrap_or(false))
            .collect();
        steps.push(Step::Family { family: used.family, variants, chars: used.chars });
    }
    steps
}

fn variant_index(v: Variant) -> usize {
    match v {
        Variant::Regular => 0,
        Variant::Bold => 1,
        Variant::Italic => 2,
        Variant::BoldItalic => 3,
    }
}

/// Phase 1: exactly the (family, variant) pairs for which the TS would fetch a face, in the TS
/// order. Empty when `!policy.embed`.
pub fn embedding_requests(usage: &[FontUsage<'_>], policy: &EmbeddingPolicy<'_>, already_embedded: &FxHashSet<String>) -> Vec<FaceRequest> {
    if !policy.embed {
        return Vec::new();
    }
    let mut requests = Vec::new();
    for step in walk(usage, policy, already_embedded) {
        if let Step::Family { family, variants, .. } = step {
            for variant in variants {
                requests.push(FaceRequest { family: family.clone(), variant });
            }
        }
    }
    requests
}

/// Phase 2: `faces` answers each request (`None`: no file). A malformed font fails the plan, as
/// the TS throw rejects it: `glyph_ids_for` and `face_info` return `Err`, never a partial plan.
/// A face whose licence forbids embedding counts as found but blocked; a face is subsetted only
/// when the policy asks and it has TrueType outlines, and `subsetted` is then true **even when**
/// the subsetter gave the input back (the TS records the intention, not the outcome).
pub fn plan_font_embedding(
    usage: &[FontUsage<'_>],
    policy: &EmbeddingPolicy<'_>,
    already_embedded: &FxHashSet<String>,
    faces: &dyn Fn(&FaceRequest) -> Option<Arc<[u8]>>,
    limits: &FontLimits,
) -> Result<EmbeddingPlan> {
    let mut plan = EmbeddingPlan::default();
    if !policy.embed {
        return Ok(plan);
    }
    for step in walk(usage, policy, already_embedded) {
        let (family, variants, chars) = match step {
            Step::Skip(family, reason) => {
                plan.skipped.push((family, reason));
                continue;
            }
            Step::Family { family, variants, chars } => (family, variants, chars),
        };
        let (mut found, mut blocked) = (0usize, 0usize);
        for variant in variants {
            let Some(bytes) = faces(&FaceRequest { family: family.clone(), variant }) else { continue };
            found += 1;
            if !is_embeddable(&bytes) {
                blocked += 1;
                continue;
            }
            let subset = policy.subset && has_true_type_outlines(&bytes);
            let data: Arc<[u8]> = if subset {
                let mut ids: Vec<u32> = chars.iter().copied().collect();
                ids.sort_unstable();
                let gids = glyph_ids_for(&bytes, ids, limits)?;
                match subset_true_type(&bytes, gids, true, limits) {
                    Cow::Borrowed(_) => bytes.clone(),
                    Cow::Owned(out) => Arc::from(out),
                }
            } else {
                bytes.clone()
            };
            let info = face_info(&bytes)?;
            plan.faces.push(FaceToEmbed { family: family.clone(), variant, bytes: data, subsetted: subset, info });
        }
        if found == 0 {
            plan.skipped.push((family, SkipReason::Unavailable));
        } else if found == blocked {
            plan.skipped.push((family, SkipReason::License));
        }
    }
    Ok(plan)
}
