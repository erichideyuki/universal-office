//! The host-configurable face budgets (LQ8(b), K15 of the uo-docx-layout plan): what a document
//! may pin and what the cache may keep unpinned, by profile.

const MIB: u64 = 1 << 20;

/// Bytes of face data (`bytes.len() + Face::heap_bytes()` per distinct face).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontBudget {
    /// what one document may pin: the faces its layouts and its registered fonts reference
    pub pinned_bytes: u64,
    /// what the cache keeps beyond the pinned faces, least recently used out first
    pub cache_bytes: u64,
}

impl FontBudget {
    /// Desktop browsers (wasm32): 128 MiB pinned per document, 64 MiB of cache.
    pub const WASM_DESKTOP: FontBudget = FontBudget { pinned_bytes: 128 * MIB, cache_bytes: 64 * MIB };

    /// Native processes (the server PDF, .NET): 1 GiB pinned, 512 MiB of cache.
    pub const NATIVE_DESKTOP: FontBudget = FontBudget { pinned_bytes: 1024 * MIB, cache_bytes: 512 * MIB };

    /// Phones: 48 MiB pinned, 16 MiB of cache. The phone target of 160 MiB counts the face bytes
    /// of the wasm side and of the browser's own font store together (K15): the shell drops its
    /// fetch buffer once the face is registered.
    pub const PHONE: FontBudget = FontBudget { pinned_bytes: 48 * MIB, cache_bytes: 16 * MIB };

    /// The desktop budget of the target: [`FontBudget::WASM_DESKTOP`] with 32-bit pointers,
    /// [`FontBudget::NATIVE_DESKTOP`] elsewhere (as `FontLimits::DEFAULT`).
    #[cfg(target_pointer_width = "32")]
    pub const DESKTOP: FontBudget = FontBudget::WASM_DESKTOP;

    /// The desktop budget of the target: [`FontBudget::WASM_DESKTOP`] with 32-bit pointers,
    /// [`FontBudget::NATIVE_DESKTOP`] elsewhere (as `FontLimits::DEFAULT`).
    #[cfg(not(target_pointer_width = "32"))]
    pub const DESKTOP: FontBudget = FontBudget::NATIVE_DESKTOP;

    pub const fn of(profile: FontProfile) -> FontBudget {
        match profile {
            FontProfile::Desktop => FontBudget::DESKTOP,
            FontProfile::Phone => FontBudget::PHONE,
        }
    }
}

/// The profile a binding picks for the device it runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontProfile {
    Desktop,
    Phone,
}
