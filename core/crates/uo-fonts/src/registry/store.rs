//! `FontRegistry`: the faces the host pushes, the misses it is asked to fetch, and the named
//! provider policies that answer `face(family, bold, italic)` from them (LQ8).
//!
//! The browser shell resolves fonts asynchronously (the `FontResolver` of `shared/fonts`: host
//! fonts, Local Font Access, the bundled open fonts) and the engine asks synchronously over what
//! is loaded. The push model keeps that shape without callbacks into the shell: the host pushes
//! what `resolver.getWithFile(family)` answers ([`FontRegistry::push_family`]) and the hosted
//! asset files ([`FontRegistry::push_asset`]); the `viewer` policy notes what the engine asked for
//! and did not find ([`FontRegistry::take_missing`], the TS `loadMissing`), the host fetches it,
//! pushes it and lays out again.
//!
//! Face bytes live once: the pushed buffer is the `Arc<[u8]>` the cache parses, the PDF embeds
//! and the host may keep. Every pushed face is charged to the registry's [`FaceLedger`] (the
//! document budget, by content, so two families over one file cost once) and a push past it is
//! refused. A face is parsed on the first answer that needs it; a file that does not parse is
//! skipped there, as the TS `try`/`catch` around `new SfntFont` does.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use super::advance::AdvanceTable;
use super::budget::{FontBudget, FontProfile};
use super::cache::{CachedFace, ContentKey, FaceCache};
use super::ledger::{FaceLedger, Ticket};
use super::ttc_face::ttc_face_by_index;
use crate::sfnt::is_font_file;
use crate::variant::{VARIANTS, Variant};
use uo_jsnum::text::to_lowercase;

/// Where the `FontResolver` found a family (shared/fonts types.ts `FontSource`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontSource {
    Host,
    LocalAccess,
    Bundled,
    Page,
    Installed,
    Substitute,
    /// the default family the resolver puts in place of one it cannot give a file for: the
    /// `viewer` policy answers `None` so that the engine follows its own chain
    Default,
}

/// One face pushed by the host.
pub struct HostFace {
    pub variant: Variant,
    pub bytes: Arc<[u8]>,
    /// the bytes are a `.ttc` and this is the face wanted (the registry extracts it, by index)
    pub ttc_index: Option<u32>,
    /// the empirical advances of the face (`*-advances.json`), read only by the PDF (PQ1)
    pub advances: Option<Arc<AdvanceTable>>,
}

impl HostFace {
    pub fn new(variant: Variant, bytes: Arc<[u8]>) -> HostFace {
        HostFace { variant, bytes, ttc_index: None, advances: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushError {
    /// the document's pinned budget cannot take it: the face stays a miss of the provider
    OverBudget,
    /// the `.ttc` face could not be extracted
    Unreadable,
    /// an asset that is not a font file (the TS `isFontFile`) or does not parse
    NotAFont,
}

/// A pushed face before it is charged.
struct Prepared {
    variant: Variant,
    bytes: Arc<[u8]>,
    key: ContentKey,
    advances: Option<Arc<AdvanceTable>>,
}

/// A pushed face: charged to the ledger at the push, parsed on first use.
pub(crate) struct Pushed {
    pub(crate) bytes: Arc<[u8]>,
    pub(crate) ticket: Ticket,
    pub(crate) advances: Option<Arc<AdvanceTable>>,
    parsed: OnceLock<Option<CachedFace>>,
}

impl Pushed {
    /// The parsed face, or `None` when the file does not parse (remembered).
    pub(crate) fn face(&self, cache: &FaceCache) -> Option<CachedFace> {
        self.parsed.get_or_init(|| cache.get_or_parse(Arc::clone(&self.bytes)).ok()).clone()
    }
}

/// A pushed face with its parse, ready to become an answer.
pub(crate) struct PushedFace {
    pub(crate) face: CachedFace,
    pub(crate) advances: Option<Arc<AdvanceTable>>,
    pub(crate) ticket: Ticket,
}

#[derive(Clone)]
pub(crate) struct Family {
    /// WordCraft study (fields, C5): the family as the host pushed it (the key is lower-cased)
    pub(crate) name: String,
    pub(crate) source: FontSource,
    pub(crate) embeddable: bool,
    pub(crate) faces: [Option<Arc<Pushed>>; 4],
}

#[derive(Clone, Default)]
pub(crate) struct Assets {
    pub(crate) faces: [Option<Arc<Pushed>>; 4],
}

/// The variants the engine asked for and no pushed face had (the TS `missing` map).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingFamily {
    /// the family as the engine wrote it the first time
    pub family: String,
    pub variants: Vec<Variant>,
}

/// Families noted as missing at most (M5); later ones are not recorded until the host takes them.
pub const MAX_MISSING_FAMILIES: usize = 4096;
/// Pairs remembered as already requested (the TS `attempted`); past it the set starts again.
pub const MAX_ATTEMPTED: usize = 65_536;

#[derive(Default)]
struct State {
    families: HashMap<String, Family>,
    assets: HashMap<String, Assets>,
    missing: Vec<MissingFamily>,
    attempted: HashSet<String>,
    generation: u64,
    /// WordCraft study (fields, C5): the full-name index of the pushed families, built for this generation
    full_names: Option<(u64, Arc<[FullNameEntry]>)>,
}

/// WordCraft study (fields, C5): one family the full-name index reads — its key, its spelling as pushed (`None` for a
/// hosted asset: the face's own) and its faces.
type IndexSource = (String, Option<String>, [Option<Arc<Pushed>>; 4]);

/// WordCraft study (fields, C5): the full names (name ID 4, platform 3) of one pushed face, by language, lower-cased.
struct FullNameEntry {
    names: Vec<(u16, String)>,
    family: String,
    variant: Variant,
}

fn slot(v: Variant) -> usize {
    VARIANTS.iter().position(|x| *x == v).unwrap_or(0)
}

/// See the module documentation. Shared by `Arc`; every method takes `&self`.
pub struct FontRegistry {
    cache: Arc<FaceCache>,
    ledger: Arc<FaceLedger>,
    budget: FontBudget,
    source: Option<Arc<dyn super::answer::FaceSource>>,
    state: Mutex<State>,
}

impl FontRegistry {
    /// A registry whose document may pin `budget.pinned_bytes`, over a (usually shared) cache.
    pub fn new(budget: FontBudget, cache: Arc<FaceCache>) -> FontRegistry {
        FontRegistry { cache, ledger: FaceLedger::new(budget.pinned_bytes), budget, source: None, state: Mutex::new(State::default()) }
    }

    pub fn with_profile(profile: FontProfile, cache: Arc<FaceCache>) -> FontRegistry {
        FontRegistry::new(FontBudget::of(profile), cache)
    }

    /// Gives the `oracle` and `server-substitute` policies their index of machine fonts.
    #[must_use]
    pub fn with_source(mut self, source: Arc<dyn super::answer::FaceSource>) -> FontRegistry {
        self.source = Some(source);
        self
    }

    pub fn budget(&self) -> FontBudget {
        self.budget
    }

    pub fn cache(&self) -> &Arc<FaceCache> {
        &self.cache
    }

    pub fn ledger(&self) -> &Arc<FaceLedger> {
        &self.ledger
    }

    pub(crate) fn source(&self) -> Option<&Arc<dyn super::answer::FaceSource>> {
        self.source.as_ref()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Bumped by every push and removal: a resolver cache kept across layout calls (milestone 2)
    /// is valid while it is equal.
    pub fn generation(&self) -> u64 {
        self.lock().generation
    }

    /// Bytes of pushed faces charged to the document budget now.
    pub fn pinned_bytes(&self) -> u64 {
        self.ledger.used()
    }

    /// The bytes of a pushed face (the face of a collection extracted) and their content key: the
    /// work that costs, done before any lock is taken.
    fn prepare(&self, face: HostFace) -> Result<Prepared, PushError> {
        let HostFace { variant, bytes, ttc_index, advances } = face;
        let bytes = match ttc_index {
            Some(index) => Arc::from(ttc_face_by_index(&bytes, index, self.cache.limits()).map_err(|_| PushError::Unreadable)?),
            None => bytes,
        };
        let key = ContentKey::of(&bytes);
        Ok(Prepared { variant, bytes, key, advances })
    }

    fn charge(&self, p: Prepared) -> Result<(Variant, Arc<Pushed>), PushError> {
        let ticket = self.ledger.charge(p.key, p.bytes.len() as u64).ok_or(PushError::OverBudget)?;
        Ok((p.variant, Arc::new(Pushed { bytes: p.bytes, ticket, advances: p.advances, parsed: OnceLock::new() })))
    }

    /// What `resolver.getWithFile(family)` answers for a family: where it came from, whether its
    /// licence lets the PDF embed it, and the faces found (a variant pushed twice keeps the last).
    /// Replaces the family; the new faces are charged while the old ones are still held, so a
    /// replacement that fits only without the old family is refused (`remove_family` first); on an
    /// error nothing changes.
    pub fn push_family(&self, family: &str, source: FontSource, embeddable: bool, faces: Vec<HostFace>) -> Result<(), PushError> {
        let prepared = faces.into_iter().map(|f| self.prepare(f)).collect::<Result<Vec<_>, _>>()?;
        let mut entry = Family { name: family.to_owned(), source, embeddable, faces: [None, None, None, None] };
        let mut state = self.lock();
        for p in prepared {
            let (variant, pushed) = self.charge(p)?;
            entry.faces[slot(variant)] = Some(pushed);
        }
        let old = state.families.insert(to_lowercase(family), entry);
        state.generation += 1;
        drop(state);
        drop(old);
        Ok(())
    }

    /// One face of the hosted asset files (the viewer's Liberation Sans, Aptos, Calibri next to
    /// the page): looked up before the resolver, answered under the family as asked. Like the TS,
    /// a buffer that is not a font file, or does not parse, is skipped; the last push of a
    /// (family, variant) wins.
    pub fn push_asset(&self, family: &str, face: HostFace) -> Result<(), PushError> {
        if !is_font_file(&face.bytes) {
            return Err(PushError::NotAFont);
        }
        let prepared = self.prepare(face)?;
        let at = slot(prepared.variant);
        let (_, pushed) = self.charge(prepared)?;
        if pushed.face(&self.cache).is_none() {
            return Err(PushError::NotAFont);
        }
        let mut state = self.lock();
        let old = state.assets.entry(to_lowercase(family)).or_default().faces[at].replace(pushed);
        state.generation += 1;
        drop(state);
        drop(old);
        Ok(())
    }

    pub fn remove_family(&self, family: &str) -> bool {
        let mut state = self.lock();
        let removed = state.families.remove(&to_lowercase(family)).is_some();
        state.generation += 1;
        removed
    }

    pub fn remove_asset(&self, family: &str) -> bool {
        let mut state = self.lock();
        let removed = state.assets.remove(&to_lowercase(family)).is_some();
        state.generation += 1;
        removed
    }

    /// Drops every pushed face, the missing notes and the attempts (a new document).
    pub fn clear(&self) {
        let mut state = self.lock();
        *state = State { generation: state.generation + 1, ..State::default() };
    }

    /// The pushed face parsed, or `None` when the file does not parse.
    pub(crate) fn pushed_face(&self, pushed: &Pushed) -> Option<PushedFace> {
        let face = pushed.face(&self.cache)?;
        Some(PushedFace { face, advances: pushed.advances.clone(), ticket: pushed.ticket.clone() })
    }

    pub(crate) fn family(&self, family: &str) -> Option<Family> {
        self.lock().families.get(&to_lowercase(family)).cloned()
    }

    /// WordCraft study (fields, C5; measured in Word 365 pt-BR, see `FaceSource::full_name` of the system index): the
    /// pushed face whose FULL name (name ID 4) is `name` in the UI language `ui_lcid` — or in en-US when the face has
    /// no record in that language ("Arial Negrito" in pt-BR is Arial Bold, "Calibri Bold" is Calibri Bold) — as
    /// `(family as pushed, bold, italic)`. Only faces that are the family they were pushed for count (name ID 1 or 16
    /// equal to it, so a substitute or an asset under another family's name does not answer for its own names); the
    /// `Substitute` and `Default` answers are skipped. The pushed families in key order, then the hosted assets (under
    /// the face's own spelling of its family); faces in variant order. The index is built once per generation. Only
    /// what was pushed is indexed: a full name whose family the document names nowhere else is found once the host
    /// has pushed that family.
    pub fn full_name_face(&self, name: &str, ui_lcid: u16) -> Option<(String, bool, bool)> {
        let want = to_lowercase(uo_jsnum::text::trim(name));
        if want.is_empty() {
            return None;
        }
        for e in self.full_name_index().iter() {
            let lang = if e.names.iter().any(|(l, _)| *l == ui_lcid) { ui_lcid } else { 0x0409 };
            if e.names.iter().any(|(l, n)| *l == lang && *n == want) {
                return Some((e.family.clone(), crate::variant::is_bold_variant(e.variant), crate::variant::is_italic_variant(e.variant)));
            }
        }
        None
    }

    fn full_name_index(&self) -> Arc<[FullNameEntry]> {
        let (generation, mut families) = {
            let state = self.lock();
            if let Some((g, idx)) = &state.full_names
                && *g == state.generation
            {
                return Arc::clone(idx);
            }
            // (the pushed families, then the hosted assets: `None` = the asset's own spelling of its family)
            let mut fams: Vec<IndexSource> = state
                .families
                .iter()
                .filter(|(_, f)| !matches!(f.source, FontSource::Substitute | FontSource::Default))
                .map(|(k, f)| (k.clone(), Some(f.name.clone()), f.faces.clone()))
                .collect();
            fams.sort_by(|a, b| a.0.cmp(&b.0));
            let mut assets: Vec<IndexSource> = state.assets.iter().map(|(k, a)| (k.clone(), None, a.faces.clone())).collect();
            assets.sort_by(|a, b| a.0.cmp(&b.0));
            fams.extend(assets);
            (state.generation, fams)
        };
        let names_of = |bytes: &[u8], id: u16| -> Vec<(u16, String)> {
            crate::info::platform3_names(bytes, id)
                .unwrap_or_default()
                .into_iter()
                .map(|(l, u)| (l, uo_jsnum::text::trim(&String::from_utf16_lossy(&u)).to_owned()))
                .collect()
        };
        let mut entries = Vec::new();
        for (key, name, faces) in families.drain(..) {
            for (i, pushed) in faces.iter().enumerate() {
                let Some(p) = pushed else { continue };
                let Some((_, own)) = names_of(&p.bytes, 1).into_iter().chain(names_of(&p.bytes, 16)).find(|(_, n)| to_lowercase(n) == key)
                else {
                    continue;
                };
                let names: Vec<(u16, String)> = names_of(&p.bytes, 4).into_iter().map(|(l, n)| (l, to_lowercase(&n))).collect();
                if !names.is_empty() {
                    let family = name.clone().unwrap_or(own);
                    entries.push(FullNameEntry { names, family, variant: VARIANTS.get(i).copied().unwrap_or(Variant::Regular) });
                }
            }
        }
        let idx: Arc<[FullNameEntry]> = entries.into();
        let mut state = self.lock();
        if state.generation == generation {
            state.full_names = Some((generation, Arc::clone(&idx)));
        }
        idx
    }

    pub(crate) fn assets(&self, family: &str) -> Option<Assets> {
        self.lock().assets.get(&to_lowercase(family)).cloned()
    }

    /// The viewer's `note(family, variant)`: remembers that the engine asked for a face that is
    /// not there.
    pub(crate) fn note_missing(&self, family: &str, variant: Variant) {
        let mut state = self.lock();
        if let Some(entry) = state.missing.iter_mut().find(|m| m.family == family) {
            if !entry.variants.contains(&variant) {
                entry.variants.push(variant);
            }
        } else if state.missing.len() < MAX_MISSING_FAMILIES {
            state.missing.push(MissingFamily { family: family.to_owned(), variants: vec![variant] });
        }
    }

    /// The notes as they stand (the TS `missing` map), without taking them.
    pub fn missing(&self) -> Vec<MissingFamily> {
        self.lock().missing.clone()
    }

    /// The TS `loadMissing` selection: what the engine asked for and no face answered, minus what
    /// was already requested (a pair is requested once per registry), in the order it was noted.
    /// Empties the notes. The host fetches these, pushes them and lays out again.
    pub fn take_missing(&self) -> Vec<MissingFamily> {
        let mut state = self.lock();
        let noted = std::mem::take(&mut state.missing);
        let mut want: Vec<MissingFamily> = Vec::new();
        for m in noted {
            let mut variants = Vec::new();
            for v in m.variants {
                if state.attempted.len() >= MAX_ATTEMPTED {
                    state.attempted.clear();
                }
                if state.attempted.insert(format!("{}|{}", to_lowercase(&m.family), v.name())) {
                    variants.push(v);
                }
            }
            if !variants.is_empty() {
                want.push(MissingFamily { family: m.family, variants });
            }
        }
        want
    }

    /// Faces pushed, for tests and diagnostics: families and assets.
    pub fn counts(&self) -> (usize, usize) {
        let state = self.lock();
        (state.families.len(), state.assets.len())
    }
}
