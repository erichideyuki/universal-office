//! The per-layout resource tables (RA2): faces, images, text effects, page fills, custom paths
//! and the interned strings, with dense ids in first-use order.

use std::sync::Arc;

use crate::ids::{CustId, FaceId, ImageId, PageFillId, Sym, TextFxId};
use crate::sym::SymbolTable;
use crate::types::{CustPath, PageFill, ResolvedTextFx};

/// A face of the layout: the TS `FontFace` the provider answered (its identity is the
/// [`FaceId`], layout K2a) with what the painters read of it.
#[derive(Clone)]
pub struct FaceRes {
    /// `key` (effective family + variant; the PDF embeds by it, so a stand-in and the real face
    /// of the same file share one font object, as in the TS)
    pub key: Sym,
    /// `family` (effective, after substitution)
    pub family: Sym,
    pub bold: bool,
    pub italic: bool,
    pub synth_bold: bool,
    pub synth_italic: bool,
    pub stand_in: bool,
    /// `font`: the parsed face (metrics, cmap, outlines, COLR). `None` only in values built
    /// without font bytes (the TS adapter of R-P2c, decoders).
    pub face: Option<uo_fonts::Face>,
    /// `bytes`: the file to embed in a PDF; `None` = not embeddable (the TS `bytes` absent).
    pub bytes: Option<Arc<[u8]>>,
    /// the empirical advances the host loaded with the face, read only by the PDF word spaces
    /// (PQ1; none on the server, as C#)
    pub advances: Option<Arc<uo_fonts::registry::AdvanceTable>>,
}

impl std::fmt::Debug for FaceRes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaceRes")
            .field("key", &self.key)
            .field("family", &self.family)
            .field("bold", &self.bold)
            .field("italic", &self.italic)
            .field("synth_bold", &self.synth_bold)
            .field("synth_italic", &self.synth_italic)
            .field("stand_in", &self.stand_in)
            .field("face", &self.face.is_some())
            .field("bytes", &self.bytes.as_ref().map(|b| b.len()))
            .field("advances", &self.advances.is_some())
            .finish()
    }
}

/// An image of the layout (RQ3: one per TS `InlineImage` object; bytes shared by content).
#[derive(Clone, Debug)]
pub struct ImageRes {
    /// `data`
    pub bytes: Arc<[u8]>,
    /// `ext`
    pub ext: Sym,
    /// `svg` (the viewer paints the vector version when present, universal-document-viewer.ts:962)
    pub svg: Option<Arc<[u8]>>,
    /// `effects` (`a:lum`, grayscale, bi-level… applied to the pixels by both painters)
    pub effects: Option<uo_drawingml::ImageEffects>,
    /// `widthTwips`, `heightTwips` of the TS object (page-fill images are built with them)
    pub width_twips: f64,
    pub height_twips: f64,
    /// `imagePixelSize` (blip tiles), computed by uo-image when the producer registers it
    pub natural_px: Option<(u32, u32)>,
}

/// The resource tables of one layout.
#[derive(Clone, Debug, Default)]
pub struct Resources {
    pub symbols: SymbolTable,
    pub faces: Vec<FaceRes>,
    pub images: Vec<ImageRes>,
    pub text_fx: Vec<ResolvedTextFx>,
    pub page_fills: Vec<PageFill>,
    pub cust: Vec<Arc<[CustPath]>>,
}

macro_rules! table {
    ($add:ident, $get:ident, $field:ident, $ty:ty, $id:ident) => {
        /// Appends an entry and returns its id (`None` past `u32::MAX - 1` entries).
        pub fn $add(&mut self, v: $ty) -> Option<$id> {
            let id = $id::from_index(self.$field.len())?;
            self.$field.push(v);
            Some(id)
        }

        /// The entry of `id`.
        #[must_use]
        pub fn $get(&self, id: $id) -> Option<&$ty> {
            self.$field.get(id.index())
        }
    };
}

impl Resources {
    #[must_use]
    pub fn new() -> Resources {
        Resources::default()
    }

    table!(add_face, face, faces, FaceRes, FaceId);
    table!(add_image, image, images, ImageRes, ImageId);
    table!(add_text_fx, text_fx, text_fx, ResolvedTextFx, TextFxId);
    table!(add_page_fill, page_fill, page_fills, PageFill, PageFillId);
    table!(add_cust, cust, cust, Arc<[CustPath]>, CustId);

    /// The code units of a symbol.
    #[must_use]
    pub fn sym(&self, id: Sym) -> &[u16] {
        self.symbols.get(id)
    }
}
