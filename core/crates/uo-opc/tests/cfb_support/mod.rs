//! A minimal Compound File writer for the CFB tests: a port of the TS test helper
//! `write/packages/docx-core/test/helpers/cfb-writer.ts` (`buildCfb`), generalized to any sector
//! size, nested storages and explicit tree links, and returning the layout so tests can patch
//! FAT, miniFAT, directory and header fields.
#![allow(dead_code)]

pub const ENDC: u32 = 0xffff_fffe;
pub const FREE: u32 = 0xffff_ffff;
pub const FATSECT: u32 = 0xffff_fffd;

/// A directory entry; node `i` of `Spec::nodes` is directory id `i + 1` (0 is the root).
#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    /// 1 storage, 2 stream (anything else is written as is)
    pub kind: u8,
    pub data: Vec<u8>,
    pub left: u32,
    pub right: u32,
    pub child: u32,
}

impl Node {
    pub fn stream(name: &str, data: &[u8]) -> Node {
        Node { name: name.into(), kind: 2, data: data.to_vec(), left: FREE, right: FREE, child: FREE }
    }
    pub fn storage(name: &str) -> Node {
        Node { name: name.into(), kind: 1, data: Vec::new(), left: FREE, right: FREE, child: FREE }
    }
    pub fn right(mut self, id: u32) -> Node {
        self.right = id;
        self
    }
    pub fn left(mut self, id: u32) -> Node {
        self.left = id;
        self
    }
    pub fn child(mut self, id: u32) -> Node {
        self.child = id;
        self
    }
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub major: u16,
    pub sector_shift: u16,
    pub mini_shift: u16,
    pub mini_cutoff: u32,
    pub root_child: u32,
    pub nodes: Vec<Node>,
}

impl Spec {
    /// v3 (512-byte sectors, 64-byte mini sectors, cutoff 4096), like the TS helper.
    pub fn v3(root_child: u32, nodes: Vec<Node>) -> Spec {
        Spec { major: 3, sector_shift: 9, mini_shift: 6, mini_cutoff: 4096, root_child, nodes }
    }
    /// v4 (4096-byte sectors).
    pub fn v4(root_child: u32, nodes: Vec<Node>) -> Spec {
        Spec { major: 4, sector_shift: 12, mini_shift: 6, mini_cutoff: 4096, root_child, nodes }
    }
}

/// The built file and where things are.
#[derive(Clone, Debug)]
pub struct Built {
    pub bytes: Vec<u8>,
    pub shift: u32,
    pub fat_sectors: u32,
    pub dir_start: u32,
    pub mini_fat_start: u32,
    pub mini_stream_start: u32,
    /// start sector of each node (FAT or miniFAT), `ENDC` for empty streams and storages
    pub starts: Vec<u32>,
}

impl Built {
    pub fn sector_size(&self) -> usize {
        1 << self.shift
    }
    pub fn sector_offset(&self, s: u32) -> usize {
        (s as usize + 1) << self.shift
    }
    /// Offset of directory entry `id` (the directory sectors are contiguous).
    pub fn dir_entry(&self, id: u32) -> usize {
        self.sector_offset(self.dir_start) + id as usize * 128
    }
    /// Offset of `FAT[i]` (the FAT sectors are 0..fat_sectors, contiguous).
    pub fn fat_entry(&self, i: u32) -> usize {
        self.sector_offset(0) + i as usize * 4
    }
    /// Offset of `miniFAT[i]` (contiguous).
    pub fn mini_fat_entry(&self, i: u32) -> usize {
        self.sector_offset(self.mini_fat_start) + i as usize * 4
    }
    pub fn set_u32(&mut self, off: usize, v: u32) {
        self.bytes[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    pub fn set_u16(&mut self, off: usize, v: u16) {
        self.bytes[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }
}

pub fn build(spec: &Spec) -> Built {
    let ss = 1usize << spec.sector_shift;
    let ms = 1usize << spec.mini_shift;
    let cutoff = spec.mini_cutoff as usize;
    let per = ss / 4;
    // mini stream
    let mut mini: Vec<u8> = Vec::new();
    let mut mini_fat: Vec<u32> = Vec::new();
    let mut starts = vec![ENDC; spec.nodes.len()];
    for (i, n) in spec.nodes.iter().enumerate() {
        let len = n.data.len();
        if n.kind != 2 || len >= cutoff || len == 0 {
            continue;
        }
        let first = (mini.len() / ms) as u32;
        let cnt = len.div_ceil(ms);
        starts[i] = first;
        for k in 0..cnt {
            mini_fat.push(if k == cnt - 1 { ENDC } else { first + k as u32 + 1 });
        }
        let mut padded = vec![0u8; cnt * ms];
        padded[..len].copy_from_slice(&n.data);
        mini.extend_from_slice(&padded);
    }
    // data sectors after the FAT: [directory][miniFAT][mini stream][large streams]
    let mut data: Vec<Vec<u8>> = Vec::new();
    let mut chains: Vec<u32> = Vec::new(); // relative FAT entries of data sectors
    let mut alloc = |bytes: &[u8], data: &mut Vec<Vec<u8>>| -> u32 {
        let first = data.len() as u32;
        let cnt = bytes.len().div_ceil(ss).max(1);
        for k in 0..cnt {
            let mut s = vec![0u8; ss];
            let from = k * ss;
            let to = bytes.len().min(from + ss);
            if from < to {
                s[..to - from].copy_from_slice(&bytes[from..to]);
            }
            data.push(s);
            chains.push(if k == cnt - 1 { ENDC } else { first + k as u32 + 1 });
        }
        first
    };
    let dir_entries = 1 + spec.nodes.len();
    let per_dir = ss / 128;
    let dir_slots = dir_entries.div_ceil(per_dir) * per_dir;
    let mut dir = vec![0u8; dir_entries.div_ceil(per_dir) * ss];
    let dir_rel = alloc(&dir, &mut data);
    let mini_fat_rel = if mini_fat.is_empty() {
        None
    } else {
        let mut mf = vec![0xffu8; mini_fat.len().div_ceil(per) * ss];
        for (i, v) in mini_fat.iter().enumerate() {
            mf[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        Some((alloc(&mf, &mut data), mf.len() / ss))
    };
    let mini_rel = if mini.is_empty() { None } else { Some(alloc(&mini, &mut data)) };
    let mut big_rel = vec![None; spec.nodes.len()];
    for (i, n) in spec.nodes.iter().enumerate() {
        if n.kind == 2 && n.data.len() >= cutoff {
            big_rel[i] = Some(alloc(&n.data, &mut data));
        }
    }
    // FAT sectors first: the smallest k whose entries cover the k FAT sectors and the data
    let d = data.len();
    let mut k = 1;
    while k * per < k + d {
        k += 1;
    }
    assert!(k <= 109, "the test writer keeps the FAT in the header DIFAT");
    let k32 = k as u32;
    let abs = |rel: u32| rel + k32;
    let mut fat: Vec<u32> = vec![FATSECT; k];
    fat.extend(chains.iter().map(|&c| if c == ENDC { ENDC } else { abs(c) }));
    for (i, n) in spec.nodes.iter().enumerate() {
        if let Some(r) = big_rel[i] {
            starts[i] = abs(r);
        } else if n.kind != 2 || n.data.is_empty() {
            starts[i] = ENDC;
        }
    }
    let mini_stream_start = mini_rel.map_or(ENDC, abs);
    // directory: root + nodes
    let mut entry = |i: usize, name: &str, kind: u8, start: u32, size: u32, child: u32, left: u32, right: u32| {
        let o = i * 128;
        for (j, u) in name.encode_utf16().enumerate().take(31) {
            dir[o + j * 2..o + j * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
        let name_len = ((name.encode_utf16().count().min(31) + 1) * 2) as u16;
        dir[o + 0x40..o + 0x42].copy_from_slice(&name_len.to_le_bytes());
        dir[o + 0x42] = kind;
        dir[o + 0x44..o + 0x48].copy_from_slice(&left.to_le_bytes());
        dir[o + 0x48..o + 0x4c].copy_from_slice(&right.to_le_bytes());
        dir[o + 0x4c..o + 0x50].copy_from_slice(&child.to_le_bytes());
        dir[o + 0x74..o + 0x78].copy_from_slice(&start.to_le_bytes());
        dir[o + 0x78..o + 0x7c].copy_from_slice(&size.to_le_bytes());
    };
    entry(0, "Root Entry", 5, mini_stream_start, mini.len() as u32, spec.root_child, FREE, FREE);
    for (i, n) in spec.nodes.iter().enumerate() {
        entry(i + 1, &n.name, n.kind, starts[i], n.data.len() as u32, n.child, n.left, n.right);
    }
    for i in dir_entries..dir_slots {
        entry(i, "", 0, 0, 0, FREE, FREE, FREE);
    }
    for (j, chunk) in dir.chunks(ss).enumerate() {
        data[dir_rel as usize + j].copy_from_slice(chunk);
    }
    // FAT sectors
    let mut fat_bytes = vec![0xffu8; k * ss];
    for (i, v) in fat.iter().enumerate() {
        fat_bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    // header (the first sector; v4 pads it to 4096)
    let mut hdr = vec![0u8; ss];
    let put16 = |h: &mut Vec<u8>, o: usize, v: u16| h[o..o + 2].copy_from_slice(&v.to_le_bytes());
    let put32 = |h: &mut Vec<u8>, o: usize, v: u32| h[o..o + 4].copy_from_slice(&v.to_le_bytes());
    hdr[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    put16(&mut hdr, 0x18, 0x3e);
    put16(&mut hdr, 0x1a, spec.major);
    put16(&mut hdr, 0x1c, 0xfffe);
    put16(&mut hdr, 0x1e, spec.sector_shift);
    put16(&mut hdr, 0x20, spec.mini_shift);
    put32(&mut hdr, 0x2c, k32);
    put32(&mut hdr, 0x30, abs(dir_rel));
    put32(&mut hdr, 0x38, spec.mini_cutoff);
    put32(&mut hdr, 0x3c, mini_fat_rel.map_or(ENDC, |(r, _)| abs(r)));
    put32(&mut hdr, 0x40, mini_fat_rel.map_or(0, |(_, n)| n as u32));
    put32(&mut hdr, 0x44, ENDC);
    put32(&mut hdr, 0x48, 0);
    for i in 0..109 {
        put32(&mut hdr, 0x4c + i * 4, if i < k { i as u32 } else { FREE });
    }
    let mut out = hdr;
    out.extend_from_slice(&fat_bytes);
    for s in &data {
        out.extend_from_slice(s);
    }
    Built {
        bytes: out,
        shift: spec.sector_shift as u32,
        fat_sectors: k32,
        dir_start: abs(dir_rel),
        mini_fat_start: mini_fat_rel.map_or(ENDC, |(r, _)| abs(r)),
        mini_stream_start,
        starts,
    }
}

/// Right-sibling chain of streams under the root (the TS `buildCfb` tree shape).
pub fn root_streams(streams: &[(&str, &[u8])]) -> (u32, Vec<Node>) {
    let n = streams.len() as u32;
    let nodes = streams
        .iter()
        .enumerate()
        .map(|(i, (name, d))| Node::stream(name, d).right(if (i as u32) + 2 <= n { i as u32 + 2 } else { FREE }))
        .collect();
    (if n > 0 { 1 } else { FREE }, nodes)
}

/// The TS `buildCfb(streams)`: v3 with the streams at the root.
pub fn build_cfb(streams: &[(&str, &[u8])]) -> Vec<u8> {
    let (child, nodes) = root_streams(streams);
    build(&Spec::v3(child, nodes)).bytes
}

/// Deterministic bytes for stream contents.
pub fn pattern(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}
