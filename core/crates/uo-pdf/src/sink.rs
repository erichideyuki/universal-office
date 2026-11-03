//! Where the file goes (record PQ4). The writer emits objects as soon as every lower number is
//! written, so a sink that forwards or drops what it receives keeps the memory of a render
//! independent of the size of the file.

use crate::error::{PdfError, Result};

/// A destination for the bytes of the file, in file order.
pub trait PdfSink {
    /// Appends `bytes`. An error stops the document with [`PdfError::Sink`].
    fn write(&mut self, bytes: &[u8]) -> Result<()>;
}

impl PdfSink for Vec<u8> {
    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.extend_from_slice(bytes);
        Ok(())
    }
}

impl<S: PdfSink + ?Sized> PdfSink for &mut S {
    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        (**self).write(bytes)
    }
}

/// Collects the file into chunks of exactly `chunk` bytes (the last one may be smaller) and hands
/// each to a callback: the browser binding turns each chunk into a JS `Uint8Array` for a `Blob`
/// (wasm memory never shrinks, so what is never held never grows it). The buffer is allocated once
/// per chunk at its full size, so what the sink holds is one chunk. Call [`ChunkedSink::finish`]
/// when the document is built to hand over the last chunk.
pub struct ChunkedSink<F: FnMut(Vec<u8>) -> Result<()>> {
    buf: Vec<u8>,
    chunk: usize,
    on_chunk: F,
}

impl<F: FnMut(Vec<u8>) -> Result<()>> ChunkedSink<F> {
    /// A sink of chunks of `chunk` bytes (at least 1 KiB).
    pub fn new(chunk: usize, on_chunk: F) -> Self {
        let chunk = chunk.max(1024);
        ChunkedSink { buf: Vec::new(), chunk, on_chunk }
    }

    /// Hands over what is left and gives the callback back.
    pub fn finish(mut self) -> Result<F> {
        if !self.buf.is_empty() {
            let last = std::mem::take(&mut self.buf);
            (self.on_chunk)(last)?;
        }
        Ok(self.on_chunk)
    }
}

impl<F: FnMut(Vec<u8>) -> Result<()>> PdfSink for ChunkedSink<F> {
    fn write(&mut self, mut bytes: &[u8]) -> Result<()> {
        while !bytes.is_empty() {
            if self.buf.is_empty() {
                self.buf.reserve_exact(self.chunk);
            }
            let room = self.chunk.saturating_sub(self.buf.len());
            let (now, later) = bytes.split_at(room.min(bytes.len()));
            self.buf.extend_from_slice(now);
            bytes = later;
            if self.buf.len() >= self.chunk {
                let full = std::mem::take(&mut self.buf);
                (self.on_chunk)(full)?;
            }
        }
        Ok(())
    }
}

/// An `std::io::Write` as a sink (a file or a socket; wrap a file in a `BufWriter`: the writer
/// makes several small writes per object).
pub struct IoSink<W: std::io::Write>(pub W);

impl<W: std::io::Write> PdfSink for IoSink<W> {
    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.0.write_all(bytes).map_err(|_| PdfError::Sink)
    }
}
