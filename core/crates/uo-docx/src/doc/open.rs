//! The head of `DocReader`'s constructor (`reader.ts`): open the Compound File, refuse encrypted
//! and pre-Word 97 documents, read the FIB and the streams everything else indexes
//! (`WordDocument`, the table stream the FIB selects, and the optional `Data` stream). The
//! reader assembly (F4) borrows these streams.

use uo_opc::cfb::read_cfb;

use super::DocError;
use super::bytes::ByteReader;
use super::fib::{Fib, read_fib};

/// The streams of a `.doc` and its FIB.
#[derive(Debug, Clone)]
pub struct DocStreams {
    pub fib: Fib,
    pub word: Vec<u8>,
    pub table: Vec<u8>,
    pub data: Option<Vec<u8>>,
}

impl DocStreams {
    /// Opens the file; the checks come in the order of the TS constructor, so the first failure
    /// is the same class.
    ///
    /// # Errors
    /// [`DocError`]: a damaged Compound File, encryption, Word 6/95, a bad FIB, a missing stream.
    pub fn open(bytes: &[u8]) -> Result<DocStreams, DocError> {
        let cfb = read_cfb(bytes).map_err(DocError::Cfb)?;
        if cfb.has("EncryptionInfo") || cfb.has("EncryptedPackage") {
            return Err(DocError::Encrypted);
        }
        // a chain cycle met while reading a stream is a plain `Error` of the TS, outside any
        // try/catch of the constructor: the catch-all of `convertDocToDocx`
        let read = |name: &str| cfb.stream(name).map_err(|_| DocError::Corrupt("compound file stream"));
        let word = read("WordDocument")?.ok_or(DocError::NoWordDocument)?;
        if word.len() >= 4 {
            let w = ByteReader::new(&word);
            let ident = w.u16(0);
            let n_fib = w.u16(2);
            if (ident == 0xa5ec || ident == 0xa5dc) && n_fib < 0x00c1 && n_fib != 0 {
                return Err(DocError::PreWord97);
            }
        }
        let fib = read_fib(&word)?;
        if fib.n_fib < 0x00c1 {
            return Err(DocError::PreWord97);
        }
        if fib.f_encrypted {
            return Err(if fib.f_obfuscated { DocError::Obfuscated } else { DocError::Encrypted });
        }
        let table =
            read(if fib.f_which_tbl_stm { "1Table" } else { "0Table" })?.ok_or(DocError::NoTableStream { one: fib.f_which_tbl_stm })?;
        let data = read("Data")?;
        Ok(DocStreams { fib, word, table, data })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_a_compound_file_is_a_cfb_error() {
        assert!(matches!(DocStreams::open(b"not a compound file at all, just text"), Err(DocError::Cfb(_))));
        assert!(matches!(DocStreams::open(&[]), Err(DocError::Cfb(_))));
    }
}
