//! A strict, independent listing of a zip central directory: what a writer under test put in
//! the archive (raw name bytes, flags, method, sizes, order), without sharing code with the
//! reader under test. Meant for archives written by the engines (well-formed ones); anything
//! unexpected is an error, not a guess.

/// One central directory entry, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CentralEntry {
    pub name: Vec<u8>,
    pub flags: u16,
    pub method: u16,
    pub crc32: u32,
    pub compressed_size: u64,
    pub size: u64,
    pub local_offset: u64,
}

fn u16_at(d: &[u8], i: usize) -> Result<u16, String> {
    d.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(|| format!("truncated at {i}"))
}

fn u32_at(d: &[u8], i: usize) -> Result<u32, String> {
    d.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| format!("truncated at {i}"))
}

fn u64_at(d: &[u8], i: usize) -> Result<u64, String> {
    Ok(u64::from(u32_at(d, i)?) | (u64::from(u32_at(d, i + 4)?) << 32))
}

/// Lists the central directory of `zip` in file order (zip64 aware).
pub fn central_directory(zip: &[u8]) -> Result<Vec<CentralEntry>, String> {
    if zip.len() < 22 {
        return Err("no end of central directory".into());
    }
    let lowest = zip.len().saturating_sub(22 + 65_535);
    let mut e = zip.len() - 22;
    while u32_at(zip, e)? != 0x0605_4b50 {
        if e == lowest {
            return Err("no end of central directory".into());
        }
        e -= 1;
    }
    let mut count = u64::from(u16_at(zip, e + 10)?);
    let mut cd = u64::from(u32_at(zip, e + 16)?);
    if e >= 20 && u32_at(zip, e - 20)? == 0x0706_4b50 {
        let rec = usize::try_from(u64_at(zip, e - 12)?).map_err(|_| "zip64 record offset")?;
        if u32_at(zip, rec)? != 0x0606_4b50 {
            return Err("bad zip64 end record".into());
        }
        count = u64_at(zip, rec + 32)?;
        cd = u64_at(zip, rec + 48)?;
    }
    let mut at = usize::try_from(cd).map_err(|_| "central directory offset")?;
    let mut out = Vec::new();
    for _ in 0..count {
        if u32_at(zip, at)? != 0x0201_4b50 {
            return Err(format!("bad central header signature at {at}"));
        }
        let name_len = usize::from(u16_at(zip, at + 28)?);
        let extra_len = usize::from(u16_at(zip, at + 30)?);
        let comment_len = usize::from(u16_at(zip, at + 32)?);
        let name = zip.get(at + 46..at + 46 + name_len).ok_or("truncated name")?.to_vec();
        let mut entry = CentralEntry {
            name,
            flags: u16_at(zip, at + 8)?,
            method: u16_at(zip, at + 10)?,
            crc32: u32_at(zip, at + 16)?,
            compressed_size: u64::from(u32_at(zip, at + 20)?),
            size: u64::from(u32_at(zip, at + 24)?),
            local_offset: u64::from(u32_at(zip, at + 42)?),
        };
        let mut x = at + 46 + name_len;
        let x_end = x + extra_len;
        while x + 4 <= x_end {
            let id = u16_at(zip, x)?;
            let len = usize::from(u16_at(zip, x + 2)?);
            if id == 1 {
                let mut v = x + 4;
                if entry.size == 0xffff_ffff {
                    entry.size = u64_at(zip, v)?;
                    v += 8;
                }
                if entry.compressed_size == 0xffff_ffff {
                    entry.compressed_size = u64_at(zip, v)?;
                    v += 8;
                }
                if entry.local_offset == 0xffff_ffff {
                    entry.local_offset = u64_at(zip, v)?;
                }
            }
            x += 4 + len;
        }
        out.push(entry);
        at = x_end + comment_len;
    }
    Ok(out)
}
