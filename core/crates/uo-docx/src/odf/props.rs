//! docx-core `odf/props.ts`: local aliases for the property types of the model. ODF styles fill
//! partial subsets of these (`Partial<...>`); in the Rust model every field of both types is
//! already optional, so the aliases are the model types themselves.

use crate::model::{ParaProps, RunProps};

/// `ResolvedRunProps = RunProps`.
pub type ResolvedRunProps = RunProps;
/// `ResolvedParaProps = ParaProps`.
pub type ResolvedParaProps = ParaProps;
