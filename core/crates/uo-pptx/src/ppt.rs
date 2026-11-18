//! The `.ppt` converter (frontier B2): PowerPoint 97-2003 (binary, [MS-CFB] container,
//! [MS-PPT] records, [MS-ODRAW] drawings) → the parts of an OOXML `.pptx`, in memory. The existing
//! pipeline (the package, `read_presentation`/`read_slide`, painting, timing and playback) runs
//! unchanged over the result.
//!
//! Port of `slides/packages/slides-core/src/ppt/*` (frozen 0.9.2, `b92b60fa`): same algorithm, same
//! order of operations, same bytes per part (`core/crates/uo-pptx/tests/captures/ppt-samples.capture`).
//! Built on `uo_opc::cfb` (Slides' `cfb.ts` is Write's with other comments) and on
//! `uo_drawingml::odraw` (`Flavor::Slides`, the msospt preset map). Record of the module:
//! `core/docs/modules/uo-pptx.md`, "B2 — .ppt converter".
//!
//! Entry points: [`convert`] (parts in the shape the package store takes), [`convert_ppt_to_pptx_parts`] (`convertPptToPptx` without the ZIP: the TS zips the
//! parts and `PptxPackage.load` unzips them again; the parts are the same, in the same order) and
//! [`is_cfb`].

mod anim;
mod context;
mod error;
mod metafile;
mod notes;
mod oa;
mod pict;
mod rd;
mod reader;
mod roundtrip;
mod shapeconv;
mod text;
mod textconv;
mod to_pptx;
mod transition;
mod xmlw;

pub use context::ConvertOptions;
pub use error::PptError;
pub use reader::{PptDocument, PptSlide, read_ppt_document};
pub use to_pptx::ppt_to_pptx_parts;
pub use transition::transition_xml;

use std::sync::Arc;

use uo_opc::cfb::CfbFile;

use crate::error::PptxError;
use crate::package::RawParts;

/// Do the bytes start with the signature of a Compound File (`isCfb`)?
#[must_use]
pub fn is_cfb(bytes: &[u8]) -> bool {
    uo_opc::cfb::is_cfb(bytes)
}

/// The UTC date of the system clock (the TS reads the local date; a wasm module has no clock and
/// the host passes [`ConvertOptions::today`]).
fn clock_today() -> (i32, u32, u32) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let days = i64::try_from(secs / 86_400).unwrap_or(0);
        // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        (i32::try_from(y).unwrap_or(1970), u32::try_from(m).unwrap_or(1), u32::try_from(d).unwrap_or(1))
    }
    #[cfg(target_arch = "wasm32")]
    {
        (1970, 1, 1)
    }
}

fn encrypted() -> PptError {
    PptError::Format("Apresentação protegida por senha (criptografada) — não é possível abri-la.".to_owned())
}

/// Converts a `.ppt` (the bytes of the file) into the parts of a `.pptx`, in the order the
/// package lists them (`convertPptToPptx` + the entries of the ZIP it writes). The error messages
/// are the TS ones (Portuguese: they go to the end user): encrypted files, unsupported versions and
/// corrupt files.
pub fn convert_ppt_to_pptx_parts(bytes: &[u8], options: &ConvertOptions) -> Result<Vec<(String, Vec<u8>)>, PptError> {
    if !is_cfb(bytes) {
        return Err(PptError::Format("O arquivo não é um .ppt (Compound File) — assinatura ausente.".to_owned()));
    }
    let cfb = CfbFile::open(bytes).map_err(|e| PptError::Format(format!("Arquivo .ppt inválido ou corrompido: {e}")))?;
    if cfb.has("EncryptedPackage") || cfb.has("EncryptionInfo") {
        return Err(encrypted());
    }
    let stream = |name: &str| -> Result<Option<Vec<u8>>, PptError> { cfb.stream(name).map_err(|e| PptError::Internal(e.to_string())) };
    let Some(doc_bytes) = stream("PowerPoint Document")? else {
        if cfb.has("PP40") || cfb.has("PP97_DUALSTORAGE") {
            return Err(PptError::Format(
                "Formato do PowerPoint 95 ou anterior não é suportado (use PowerPoint 97-2003 ou .pptx).".to_owned(),
            ));
        }
        return Err(PptError::Format(
            "O arquivo não contém uma apresentação do PowerPoint (stream \"PowerPoint Document\" ausente).".to_owned(),
        ));
    };
    let current_user = stream("Current User")?;
    let pics = stream("Pictures")?;
    let doc = read_ppt_document(&doc_bytes, pics.as_deref(), current_user.as_deref())?;
    if doc.slides.is_empty() && doc.masters.is_empty() {
        return Err(PptError::Format("Apresentação .ppt sem slides legíveis.".to_owned()));
    }
    let today = options.today.unwrap_or_else(clock_today);
    let parts = ppt_to_pptx_parts(&doc, options, today)?;
    Ok(parts.into_vec())
}

/// `convertPptToPptx` (`ppt/index.ts`) for the package store: the parts of the converted package, in
/// archive order (the TS zips them and `PptxPackage.load` unzips them again; skipping that round
/// trip gives the same entries in the same order).
pub fn convert(bytes: &[u8]) -> Result<RawParts, PptxError> {
    convert_with(bytes, &ConvertOptions::default())
}

/// [`convert`] with options (`ConvertOptions::ts_strict()` for the TS-parity harnesses).
pub fn convert_with(bytes: &[u8], options: &ConvertOptions) -> Result<RawParts, PptxError> {
    let parts = convert_ppt_to_pptx_parts(bytes, options).map_err(|e| PptxError::PptFormat(e.message().into()))?;
    Ok(parts.into_iter().map(|(n, d)| (n.into_boxed_str(), Arc::<[u8]>::from(d))).collect())
}
