//! The named provider policies of the font registry (K6, LQ8 of the uo-docx-layout plan): who
//! answers `face(family, bold, italic)` and with which identities.
//!
//! | name | port of | face source | identity of an answer |
//! |---|---|---|---|
//! | `viewer` | `write/packages/viewer/src/fonts.ts:90-137` | what the host pushed ([`FontRegistry`](crate::registry::FontRegistry)) | per provider object (one per layout call), per request as asked (case-sensitive) |
//! | `server-substitute` | C# `SubstituteFontProvider` over `SystemFontProvider` (**C# is the reference**: clickCompliance calls `DocxLayoutPipeline.ToPdf(docx, SubstituteFontProvider.Shared, ...)`) | the machine's fonts ([`FaceSource`](crate::registry::FaceSource)) | per exact request while someone holds it; a metric substitute is a **fresh** answer every call |
//! | `oracle` | `write/tools/corpus-lab/node-fonts.mjs:162-181` | the machine's fonts | per lower-case request while someone holds it |
//! | `editor` | `liveFontProvider` (universal-write.ts:16087), cache across calls by `fonts.version` | the editor's loaded fonts | milestone 2: not built; the registry's [`generation`](crate::registry::FontRegistry::generation) is its `fonts.version` |
//!
//! Each provider is a [`FontProvider`]; the layout asks it from its resolver (the TS `FaceResolver`
//! cache, per layout call, sits above) and maps [`AnswerId`](crate::registry::AnswerId) to its own
//! `FaceId` in first-use order.

mod oracle;
mod server;
mod viewer;

use std::sync::Arc;

pub use oracle::OracleProvider;
pub use server::{SubstituteFontProvider, SystemFontProvider, csharp_metric_substitute};
pub use viewer::{MAX_VIEWER_REQUESTS, ViewerProvider};

use crate::registry::answers::Resolved;
use crate::registry::{AnswerParts, FaceLedger, FontAnswer, FontProvider, FontRegistry, SourceFace};

/// The answer of a picked face, charged to the document ledger when there is one: past the budget
/// the face is refused (and not remembered as missing).
pub(crate) fn from_source(src: SourceFace, bold: bool, italic: bool, ledger: Option<&Arc<FaceLedger>>) -> Resolved {
    let ticket = match ledger {
        Some(l) => match l.charge(src.face.content_key(), src.face.cost()) {
            Some(t) => Some(t),
            None => return Resolved::Refused,
        },
        None => None,
    };
    Resolved::Answer(FontAnswer::new(AnswerParts {
        key: src.key,
        family: src.family,
        face: src.face,
        bold,
        italic,
        synth_bold: src.synth_bold,
        synth_italic: src.synth_italic,
        stand_in: false,
        advances: None,
        ticket,
    }))
}

/// The policies by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolicyName {
    Viewer,
    ServerSubstitute,
    Oracle,
    /// milestone 2
    Editor,
}

impl PolicyName {
    pub const fn as_str(self) -> &'static str {
        match self {
            PolicyName::Viewer => "viewer",
            PolicyName::ServerSubstitute => "server-substitute",
            PolicyName::Oracle => "oracle",
            PolicyName::Editor => "editor",
        }
    }

    pub fn from_name(name: &str) -> Option<PolicyName> {
        [PolicyName::Viewer, PolicyName::ServerSubstitute, PolicyName::Oracle, PolicyName::Editor].into_iter().find(|p| p.as_str() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyError {
    /// `server-substitute` and `oracle` need the registry to have a font source
    NoSource,
    /// the `editor` policy belongs to milestone 2
    NotBuilt,
}

/// A provider of the named policy over the registry: for one layout call (the `viewer`'s answers
/// are remembered per provider object, so ask for a new one per call).
pub fn provider(registry: &Arc<FontRegistry>, policy: PolicyName) -> Result<Box<dyn FontProvider>, PolicyError> {
    match policy {
        PolicyName::Viewer => Ok(Box::new(ViewerProvider::new(Arc::clone(registry)))),
        PolicyName::ServerSubstitute => {
            let source = registry.source().ok_or(PolicyError::NoSource)?;
            let inner = SystemFontProvider::new(Arc::clone(source), Some(Arc::clone(registry.ledger())));
            Ok(Box::new(SubstituteFontProvider::new(Arc::new(inner))))
        }
        PolicyName::Oracle => {
            let source = registry.source().ok_or(PolicyError::NoSource)?;
            Ok(Box::new(OracleProvider::new(Arc::clone(source), Some(Arc::clone(registry.ledger())), &[])))
        }
        PolicyName::Editor => Err(PolicyError::NotBuilt),
    }
}
