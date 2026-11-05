//! Media of the package (`mediaTarget`): one file per distinct picture, named in the order the
//! pictures are first met.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use uo_drawingml::odraw::Blip;

use super::{DocxWriter, FileData};

impl<'a> DocxWriter<'a> {
    /// `mediaTarget`: the part (relative to `word/`) that holds the picture. A picture whose bytes
    /// equal those of an earlier one of the same type shares its file (the TS compared the type, the
    /// length and a hash of a sample of the bytes: decision G24, the full bytes).
    pub(super) fn media_target(&mut self, blip: Blip<'a>) -> String {
        let ext = blip.kind.ext();
        let mut h = DefaultHasher::new();
        blip.bytes.hash(&mut h);
        let key = (ext, blip.bytes.len(), h.finish());
        if let Some(names) = self.media.get(&key) {
            for name in names {
                if self.files.bytes_of(&format!("word/{name}")) == Some(blip.bytes.as_ref()) {
                    return name.clone();
                }
            }
        }
        self.media_n += 1;
        let name = format!("media/image{}.{ext}", self.media_n);
        self.files.set(format!("word/{name}"), FileData::Bytes(blip.bytes));
        if !self.media_ext.contains(&ext) {
            self.media_ext.push(ext);
        }
        self.media.entry(key).or_default().push(name.clone());
        name
    }
}
