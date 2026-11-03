//! The test parser of the structural comparison (record §6): our PDFs are classic-xref, no object
//! streams. It validates the file frame (header, every xref offset hits `N 0 obj`, every object is
//! closed, the trailer matches) and normalizes the file: per object the dictionary without
//! `/Filter /FlateDecode` and with `/Length N`, and the payload inflated.
//!
//! `normalize` is the same procedure as `normalize` of `capture_writer.mjs`.

use uo_opc::flate::inflate_zlib;
use uo_testkit::stream::Stream;

#[derive(Debug)]
pub struct Parsed<'a> {
    pub objs: Vec<&'a [u8]>,
    pub root: u32,
    pub count: u32,
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn number_at(b: &[u8], from: usize) -> Result<(usize, usize), String> {
    let n = b.get(from..).map_or(0, |s| s.iter().take_while(|c| c.is_ascii_digit()).count());
    let text = std::str::from_utf8(b.get(from..from + n).ok_or("number")?).map_err(|_| "number")?;
    Ok((text.parse().map_err(|_| "number")?, n))
}

pub fn parse_pdf(u8: &[u8]) -> Result<Parsed<'_>, String> {
    let sx = rfind(u8, b"startxref\n").ok_or("no startxref")?;
    let (xref_at, _) = number_at(u8, sx + 10)?;
    let head = u8.get(xref_at..).ok_or("startxref outside the file")?;
    if !head.starts_with(b"xref\n0 ") {
        return Err("bad xref".into());
    }
    let (count, n) = number_at(u8, xref_at + 7)?;
    let mut p = xref_at + 7 + n;
    if u8.get(p) != Some(&b'\n') {
        return Err("bad xref count line".into());
    }
    p += 1;
    if u8.get(p..p + 20) != Some(b"0000000000 65535 f \n".as_slice()) {
        return Err("bad free entry".into());
    }
    p += 20;
    let mut offs = Vec::new();
    for i in 1..count {
        let e = u8.get(p..p + 20).ok_or("short xref")?;
        let ok = e.get(..10).is_some_and(|d| d.iter().all(u8::is_ascii_digit)) && e.get(10..) == Some(b" 00000 n \n".as_slice());
        if !ok {
            return Err(format!("bad xref entry {i}"));
        }
        offs.push(number_at(u8, p)?.0);
        p += 20;
    }
    let trailer = format!("trailer\n<< /Size {count} /Root ");
    let tail = u8.get(p..).ok_or("no trailer")?;
    let t = tail.strip_prefix(trailer.as_bytes()).ok_or("bad trailer")?;
    let (root, n) = number_at(t, 0)?;
    let end = format!(" 0 R >>\nstartxref\n{xref_at}\n%%EOF\n");
    if t.get(n..) != Some(end.as_bytes()) {
        return Err("bad trailer end".into());
    }
    if !u8.starts_with(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n") {
        return Err("bad header".into());
    }
    let mut objs = Vec::new();
    for (i, &off) in offs.iter().enumerate() {
        let h = format!("{} 0 obj\n", i + 1);
        if !u8.get(off..).is_some_and(|s| s.starts_with(h.as_bytes())) {
            return Err(format!("xref entry {} does not hit its object", i + 1));
        }
        let end = offs.get(i + 1).copied().unwrap_or(xref_at).checked_sub(8).ok_or("object too short")?;
        if u8.get(end..end + 8) != Some(b"\nendobj\n".as_slice()) {
            return Err(format!("object {} not closed", i + 1));
        }
        objs.push(u8.get(off + h.len()..end).ok_or("object body")?);
    }
    Ok(Parsed { objs, root: u32::try_from(root).map_err(|_| "root")?, count: u32::try_from(count).map_err(|_| "count")? })
}

#[derive(Debug, PartialEq, Eq)]
pub struct Norm {
    pub objects: u32,
    pub deflated: u32,
    pub digest: String,
}

/// The head of a stream object: `(<< ... /Length N >>, N, bytes up to and including "stream\n")`.
fn stream_head(body: &[u8]) -> Option<(String, usize, usize)> {
    let prefix = body.get(..body.len().min(4000))?;
    let at = find(prefix, b"\nstream\n")?;
    let dict: String = prefix.get(..at)?.iter().map(|&c| char::from(c)).collect();
    let dict = dict.strip_suffix(" >>")?;
    let len_at = dict.rfind(" /Length ")?;
    let digits = dict.get(len_at + 9..)?;
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) || !dict.starts_with("<<") {
        return None;
    }
    Some((format!("{dict} >>"), digits.parse().ok()?, at + 8))
}

pub fn normalize(u8: &[u8]) -> Result<Norm, String> {
    let Parsed { objs, root, count } = parse_pdf(u8)?;
    let mut s = Stream::new();
    s.u32(count).u32(root);
    let mut deflated = 0;
    for (i, body) in objs.iter().enumerate() {
        s.tag(b'o').u32(i as u32 + 1);
        let Some((head, len, head_len)) = stream_head(body) else {
            s.tag(b'd').digest_of(body);
            continue;
        };
        let payload = body.get(head_len..head_len + len).ok_or("stream payload")?;
        if body.get(head_len + len..) != Some(b"\nendstream".as_slice()) {
            return Err(format!("object {}: bad endstream", i + 1));
        }
        let flate = head.contains(" /Filter /FlateDecode");
        let dict = head.replacen(" /Filter /FlateDecode", "", 1);
        let at = dict.rfind(" /Length ").ok_or("length")?;
        let dict = format!("{} /Length N >>", &dict[..at]);
        let data = if flate {
            deflated += 1;
            inflate_zlib(payload, None, 1 << 31).map_err(|e| format!("object {}: {e:?}", i + 1))?
        } else {
            payload.to_vec()
        };
        s.tag(b's').str(&dict).digest_of(&data);
    }
    Ok(Norm { objects: count - 1, deflated, digest: s.finish().short() })
}
