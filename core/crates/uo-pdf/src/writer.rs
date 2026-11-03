//! The low-level writer (`pdf-writer/writer.ts`, `bytes.ts`): indirect objects, dictionaries,
//! streams, the classic xref table and the trailer. No knowledge of fonts or layout.
//!
//! **Streaming (record PQ4).** The TS keeps every object body and writes the file at the end.
//! Here an object goes to the sink as soon as every lower number is written, so the file order
//! is the TS order and an object defined in number order (every image, shading and font object)
//! never waits. An object defined ahead of a lower one that is still undefined (the catalog is
//! allocated before the pages and defined last, so every page object waits for it) is held until
//! the gap closes.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use uo_opc::flate::ZlibDeflater;

use crate::error::{LimitKind, PdfError, PdfLimits, Result};
use crate::num::{Out, latin1};
use crate::sink::PdfSink;

/// A compressed stream waits for its object number; `deflated` is the decision of the TS rule
/// (more than 64 bytes and smaller deflated), made when the body was prepared.
#[derive(Debug, Default)]
pub struct StreamBody {
    payload: Vec<u8>,
    deflated: bool,
}

impl StreamBody {
    /// Bytes of the payload as they will be written.
    pub fn len(&self) -> usize {
        self.payload.len()
    }

    pub fn is_empty(&self) -> bool {
        self.payload.is_empty()
    }

    /// Whether the payload is a zlib stream (`/Filter /FlateDecode`).
    pub fn is_deflated(&self) -> bool {
        self.deflated
    }
}

/// Scratch buffers bigger than this are given back after each stream.
const SCRATCH_KEEP: usize = 1 << 20;
/// What the TS compresses only above: shorter streams are stored.
const COMPRESS_ABOVE: usize = 64;
/// The bytes around an object body: `N 0 obj\n` (at most 17 with a 10-digit number) and `\nendobj\n`.
const FRAMING_MAX: u64 = 25;
/// Entries of the xref table written per sink call.
const XREF_BLOCK: usize = 2048;

/// Writes a PDF 1.7 file to a sink, object by object.
pub struct PdfWriter<S: PdfSink> {
    sink: S,
    compress: bool,
    max_output: u64,
    /// objects allocated so far (the last number)
    next: u32,
    /// objects written to the sink (numbers 1..=written)
    written: u32,
    /// the byte offset of each written object
    offsets: Vec<u64>,
    out_len: u64,
    pending: BTreeMap<u32, Vec<u8>>,
    pending_bytes: u64,
    started: bool,
    deflater: ZlibDeflater,
    scratch: Vec<u8>,
}

impl<S: PdfSink> PdfWriter<S> {
    /// A writer; `compress` turns on `FlateDecode` for the streams that gain from it (the TS
    /// default is on).
    pub fn new(sink: S, compress: bool) -> Self {
        PdfWriter::with_limits(sink, compress, &PdfLimits::DEFAULT)
    }

    /// A writer that stops at `limits.max_output_bytes`.
    pub fn with_limits(sink: S, compress: bool, limits: &PdfLimits) -> Self {
        PdfWriter {
            sink,
            compress,
            max_output: limits.max_output_bytes,
            next: 0,
            written: 0,
            offsets: Vec::new(),
            out_len: 0,
            pending: BTreeMap::new(),
            pending_bytes: 0,
            started: false,
            // fflate's zlibSync default
            deflater: ZlibDeflater::new(6),
            scratch: Vec::new(),
        }
    }

    pub fn is_compressing(&self) -> bool {
        self.compress
    }

    /// Bytes handed to the sink so far.
    pub fn bytes_written(&self) -> u64 {
        self.out_len
    }

    /// Objects allocated so far.
    pub fn object_count(&self) -> u32 {
        self.next
    }

    /// Bytes of objects defined but waiting for a lower number.
    pub fn pending_bytes(&self) -> u64 {
        self.pending_bytes
    }

    /// Reserves an object number (1-based) for a forward reference.
    pub fn alloc(&mut self) -> Result<u32> {
        let n = self.next.checked_add(1).ok_or(PdfError::Limit(LimitKind::Objects))?;
        self.next = n;
        Ok(n)
    }

    /// Defines the body of an allocated object (what goes between `obj` and `endobj`). Defining
    /// an object that is still waiting replaces it (as the TS); one already written is an error.
    pub fn define(&mut self, num: u32, body: &[u8]) -> Result<()> {
        self.define_parts(num, &[body])
    }

    /// [`PdfWriter::define`] of a text body (one byte per UTF-16 code unit, the low 8 bits).
    pub fn define_text(&mut self, num: u32, body: &str) -> Result<()> {
        if body.is_ascii() {
            return self.define_parts(num, &[body.as_bytes()]);
        }
        let mut bytes = Vec::with_capacity(body.len());
        latin1(&mut bytes, body);
        self.define_parts(num, &[&bytes])
    }

    /// Allocates and defines a dictionary given as a `<< ... >>` text.
    pub fn add_dict(&mut self, dict: &str) -> Result<u32> {
        let n = self.alloc()?;
        self.define_text(n, dict)?;
        Ok(n)
    }

    /// Allocates and defines a stream object. `dict` holds the extra entries of the dictionary
    /// (no `/Length`, no `<<` `>>`). With compression on and `allow_compress`, a stream of more
    /// than 64 bytes is deflated (zlib, RFC 1950) when that makes it smaller.
    pub fn add_stream(&mut self, dict: &str, data: &[u8], allow_compress: bool) -> Result<u32> {
        let n = self.alloc()?;
        self.define_stream(n, dict, data, allow_compress)?;
        Ok(n)
    }

    pub fn define_stream(&mut self, num: u32, dict: &str, data: &[u8], allow_compress: bool) -> Result<()> {
        let deflated = self.deflate_into_scratch(data, allow_compress)?;
        let scratch = std::mem::take(&mut self.scratch);
        let payload: &[u8] = if deflated { &scratch } else { data };
        let head = stream_head(dict, deflated, payload.len());
        let result = self.define_parts(num, &[&head, payload, b"\nendstream"]);
        self.scratch = scratch;
        self.release_scratch();
        result
    }

    /// Prepares the payload of a stream now (compression decided here) to define it later: a
    /// page's content, held compressed until its object number comes up.
    pub fn compress_body(&mut self, data: Vec<u8>, allow_compress: bool) -> Result<StreamBody> {
        let deflated = self.deflate_into_scratch(&data, allow_compress)?;
        let body = if deflated {
            StreamBody { payload: self.scratch.as_slice().to_vec(), deflated: true }
        } else {
            StreamBody { payload: data, deflated: false }
        };
        self.release_scratch();
        Ok(body)
    }

    /// Defines a stream object from a prepared body.
    pub fn define_body(&mut self, num: u32, dict: &str, body: StreamBody) -> Result<()> {
        let head = stream_head(dict, body.deflated, body.payload.len());
        self.define_parts(num, &[&head, &body.payload, b"\nendstream"])
    }

    fn deflate_into_scratch(&mut self, data: &[u8], allow_compress: bool) -> Result<bool> {
        if !(self.compress && allow_compress && data.len() > COMPRESS_ABOVE) {
            return Ok(false);
        }
        self.scratch.clear();
        self.deflater.zlib_into(data, &mut self.scratch).map_err(|_| PdfError::Invalid("deflate failed"))?;
        Ok(self.scratch.len() < data.len())
    }

    fn release_scratch(&mut self) {
        if self.scratch.capacity() > SCRATCH_KEEP {
            self.scratch = Vec::new();
        } else {
            self.scratch.clear();
        }
    }

    fn define_parts(&mut self, num: u32, parts: &[&[u8]]) -> Result<()> {
        if num == 0 || num > self.next {
            return Err(PdfError::Invalid("object defined before it was allocated"));
        }
        if num <= self.written {
            return Err(PdfError::Invalid("object defined twice"));
        }
        let len: u64 = parts.iter().map(|p| p.len() as u64).sum();
        let replaced = self.pending.get(&num).map_or(0, |b| b.len() as u64);
        let held = self.out_len.saturating_add(self.pending_bytes.saturating_sub(replaced));
        if held.saturating_add(len).saturating_add(FRAMING_MAX) > self.max_output {
            return Err(PdfError::Limit(LimitKind::Output));
        }
        if num == self.written + 1 {
            self.emit(parts)?;
            return self.flush_pending();
        }
        let mut body = Vec::with_capacity(usize::try_from(len).map_err(|_| PdfError::Limit(LimitKind::Output))?);
        for p in parts {
            body.extend_from_slice(p);
        }
        self.pending_bytes = self.pending_bytes.saturating_sub(replaced).saturating_add(len);
        self.pending.insert(num, body);
        Ok(())
    }

    fn flush_pending(&mut self) -> Result<()> {
        while let Some(body) = self.pending.remove(&(self.written + 1)) {
            self.pending_bytes = self.pending_bytes.saturating_sub(body.len() as u64);
            self.emit(&[&body])?;
        }
        Ok(())
    }

    fn put(&mut self, bytes: &[u8]) -> Result<()> {
        self.sink.write(bytes)?;
        self.out_len = self.out_len.saturating_add(bytes.len() as u64);
        Ok(())
    }

    /// `%PDF-1.7`, then a comment of high bytes that marks the file binary.
    fn start(&mut self) -> Result<()> {
        if !self.started {
            self.started = true;
            self.put(b"%PDF-1.7\n")?;
            self.put(&[0x25, 0xe2, 0xe3, 0xcf, 0xd3, b'\n'])?;
        }
        Ok(())
    }

    /// Writes the next object (number `written + 1`).
    fn emit(&mut self, parts: &[&[u8]]) -> Result<()> {
        self.start()?;
        let num = self.written + 1;
        self.offsets.push(self.out_len);
        let mut head = Vec::with_capacity(16);
        let _ = writeln!(Out(&mut head), "{num} 0 obj");
        self.put(&head)?;
        for p in parts {
            self.put(p)?;
        }
        self.put(b"\nendobj\n")?;
        self.written = num;
        Ok(())
    }

    /// Finishes the file with the xref table and the trailer (`root` = the `/Catalog` object) and
    /// gives the sink back. Every allocated object must be defined.
    pub fn build(mut self, root: u32) -> Result<S> {
        self.start()?;
        self.flush_pending()?;
        if self.written != self.next {
            return Err(PdfError::Invalid("an object was allocated but not defined"));
        }
        let count = u64::from(self.next) + 1;
        if self.out_len.saturating_add(count.saturating_mul(20)).saturating_add(128) > self.max_output {
            return Err(PdfError::Limit(LimitKind::Output));
        }
        let xref_offset = self.out_len;
        let mut block = Vec::with_capacity(XREF_BLOCK * 20 + 64);
        let _ = write!(Out(&mut block), "xref\n0 {count}\n0000000000 65535 f \n");
        let offsets = std::mem::take(&mut self.offsets);
        for chunk in offsets.chunks(XREF_BLOCK) {
            for off in chunk {
                let _ = writeln!(Out(&mut block), "{off:010} 00000 n ");
            }
            self.put(&block)?;
            block.clear();
        }
        let _ = write!(Out(&mut block), "trailer\n<< /Size {count} /Root {root} 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n");
        self.put(&block)?;
        Ok(self.sink)
    }
}

/// `<< {dict} /Filter /FlateDecode /Length {n} >>\nstream\n`: the head of a stream object, spaces
/// as the TS writes them (`dict.trim()` and the filter entry in between).
fn stream_head(dict: &str, deflated: bool, len: usize) -> Vec<u8> {
    let dict = uo_jsnum::text::trim(dict);
    let mut head = Vec::with_capacity(dict.len() + 56);
    head.extend_from_slice(b"<< ");
    latin1(&mut head, dict);
    if deflated {
        head.extend_from_slice(b" /Filter /FlateDecode");
    }
    let _ = write!(Out(&mut head), " /Length {len} >>\nstream\n");
    head
}
