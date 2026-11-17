//! Errors of the `.ppt` converter. The messages are the TS ones (`PptFormatError`, in Portuguese:
//! they are shown to the end user).

use std::fmt;

/// Why a `.ppt` could not be converted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PptError {
    /// `PptFormatError`: the message of the TS error
    Format(String),
    /// any other exception of the TS converter; `convertPptToPptx` turns it into a format error
    /// with the prefix of [`PptError::message`]
    Internal(String),
}

impl PptError {
    /// The message `convertPptToPptx` throws.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            PptError::Format(m) => m.clone(),
            PptError::Internal(m) => format!("Arquivo .ppt corrompido ou não suportado: {m}"),
        }
    }
}

impl fmt::Display for PptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for PptError {}
