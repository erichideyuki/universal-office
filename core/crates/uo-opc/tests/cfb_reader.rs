//! The CFB reader against files built by the test writer (port of the TS `buildCfb`), including
//! every leniency and guard of `cfb.ts`.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions

mod cfb_support;

use cfb_support::{Built, ENDC, FREE, Node, Spec, build, build_cfb, pattern, root_streams};
use uo_opc::Error;
use uo_opc::cfb::{CFB_SIGNATURE, CfbEntryKind, CfbFile, is_cfb, read_cfb};

fn open(bytes: &[u8]) -> CfbFile<'_> {
    CfbFile::open(bytes).unwrap()
}

fn read(file: &CfbFile<'_>, path: &str) -> Vec<u8> {
    file.stream(path).unwrap().unwrap()
}

fn is_cfb_error<T>(r: uo_opc::Result<T>) -> bool {
    matches!(r, Err(Error::Cfb(_)))
}

fn word_like(spec: fn(u32, Vec<Node>) -> Spec) -> (Built, Vec<(String, Vec<u8>)>) {
    let streams = vec![
        ("WordDocument".to_string(), pattern(5000, 1)),
        ("1Table".to_string(), pattern(100, 2)),
        ("\u{5}SummaryInformation".to_string(), pattern(4095, 3)),
        ("Data".to_string(), pattern(4096, 4)),
        ("Empty".to_string(), Vec::new()),
    ];
    let refs: Vec<(&str, &[u8])> = streams.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    let (child, nodes) = root_streams(&refs);
    (build(&spec(child, nodes)), streams)
}

#[test]
fn signature_check() {
    assert!(is_cfb(&CFB_SIGNATURE));
    assert!(is_cfb(&build_cfb(&[])));
    assert!(!is_cfb(&CFB_SIGNATURE[..7]));
    assert!(!is_cfb(b"PK\x03\x04 not a compound file"));
    assert!(!is_cfb(&[]));
}

#[test]
fn reads_mini_and_regular_streams_v3_and_v4() {
    for spec in [Spec::v3 as fn(u32, Vec<Node>) -> Spec, Spec::v4] {
        let (built, streams) = word_like(spec);
        let file = read_cfb(&built.bytes).unwrap();
        let entries = file.entries();
        assert_eq!(entries.len(), 1 + streams.len());
        assert_eq!(entries[0].kind, CfbEntryKind::Root);
        assert_eq!(entries[0].name, "Root Entry");
        assert_eq!(entries[0].path, "");
        for (e, (name, data)) in entries[1..].iter().zip(&streams) {
            assert_eq!(&e.name, name);
            assert_eq!(&e.path, name);
            assert_eq!(e.kind, CfbEntryKind::Stream);
            assert_eq!(e.size, data.len() as u64);
            assert_eq!(&read(&file, name), data, "{name}");
            assert!(file.has(name));
        }
        // 4095 bytes live in the mini stream, 4096 in regular sectors
        assert_eq!(entries[3].start, built.starts[2]);
        assert_eq!(entries[4].start, built.starts[3]);
        assert!(built.starts[3] >= built.fat_sectors);
    }
}

#[test]
fn lookup_is_case_insensitive_and_only_streams_are_read() {
    let mut nodes =
        vec![Node::storage("ObjectPool").child(2).right(3), Node::stream("Contents", b"abc"), Node::stream("WordDocument", b"x")];
    nodes[1].right = FREE;
    let built = build(&Spec::v3(1, nodes));
    let file = open(&built.bytes);
    assert_eq!(read(&file, "worddocument"), b"x");
    assert_eq!(read(&file, "OBJECTPOOL/contents"), b"abc");
    assert!(file.has("objectpool"));
    assert_eq!(file.stream("ObjectPool").unwrap(), None, "a storage is not a stream");
    assert_eq!(file.stream("Missing").unwrap(), None);
    // the root has no path (cfb.ts:152: it is listed but not indexed)
    assert!(!file.has(""));
    assert!(!file.has("Root Entry"));
    assert_eq!(file.stream("Empty").unwrap(), None);
}

#[test]
fn unicode_lowercase_like_js() {
    // JS toLowerCase is the Unicode mapping, not ASCII only
    let built = build(&Spec::v3(1, vec![Node::stream("ÉTÉ", b"1")]));
    let file = open(&built.bytes);
    assert_eq!(read(&file, "été"), b"1");
}

#[test]
fn tree_is_walked_in_order_with_storages_expanded_in_place() {
    // root → B (left A, right D); D is a storage with children C2 (left C1)
    let nodes = vec![
        Node::stream("A", b"a"),
        Node::stream("B", b"b").left(1).right(3),
        Node::storage("D").child(5),
        Node::stream("C1", b"c1"),
        Node::stream("C2", b"c2").left(4).right(6),
        Node::storage("E"),
    ];
    let built = build(&Spec::v3(2, nodes));
    let file = open(&built.bytes);
    let paths: Vec<&str> = file.entries().iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["", "A", "B", "D", "D/C1", "D/C2", "D/E"]);
    assert_eq!(file.entries()[6].kind, CfbEntryKind::Storage);
    assert_eq!(read(&file, "d/c2"), b"c2");
}

#[test]
fn empty_storage_name_makes_children_top_level() {
    // `prefix ? prefix/name : name` (cfb.ts:171)
    let built = build(&Spec::v3(1, vec![Node::storage("").child(2), Node::stream("S", b"s")]));
    let file = open(&built.bytes);
    let paths: Vec<&str> = file.entries().iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["", "", "S"]);
    assert!(file.has(""));
    assert_eq!(read(&file, "s"), b"s");
}

#[test]
fn duplicate_paths_keep_both_entries_and_the_last_wins_lookup() {
    let built = build(&Spec::v3(1, vec![Node::stream("X", b"first").right(2), Node::stream("x", b"second")]));
    let file = open(&built.bytes);
    assert_eq!(file.entries().len(), 3);
    assert_eq!(read(&file, "X"), b"second");
}

#[test]
fn unknown_entry_types_are_skipped_but_their_siblings_are_walked() {
    let mut odd = Node::stream("Odd", b"o").right(2);
    odd.kind = 0;
    let built = build(&Spec::v3(1, vec![odd, Node::stream("Next", b"n")]));
    let file = open(&built.bytes);
    let names: Vec<&str> = file.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Root Entry", "Next"]);
}

#[test]
fn odd_name_length_rounds_up_like_the_js_loop() {
    let mut built = build(&Spec::v3(1, vec![Node::stream("ABCD", b"1")]));
    let off = built.dir_entry(1) + 0x40;
    built.set_u16(off, 5); // 5 bytes: max(0, 2.5 - 1) = 1.5 → 2 iterations
    assert_eq!(open(&built.bytes).entries()[1].name, "AB");
    built.set_u16(off, 4); // 2 - 1 = 1
    assert_eq!(open(&built.bytes).entries()[1].name, "A");
    built.set_u16(off, 1000); // capped at 31 characters
    assert_eq!(open(&built.bytes).entries()[1].name.encode_utf16().count(), 31);
    built.set_u16(off, 0);
    assert_eq!(open(&built.bytes).entries()[1].name, "");
}

#[test]
fn v3_ignores_the_high_size_bits_and_v4_uses_them() {
    let data = pattern(5000, 9);
    for (spec, major) in [(Spec::v3 as fn(u32, Vec<Node>) -> Spec, 3), (Spec::v4, 4)] {
        let mut built = build(&spec(1, vec![Node::stream("Big", &data)]));
        built.set_u32(built.dir_entry(1) + 0x7c, 1);
        let file = open(&built.bytes);
        let got = read(&file, "Big");
        if major == 3 {
            assert_eq!(file.entries()[1].size, 5000);
            assert_eq!(got, data);
        } else {
            assert_eq!(file.entries()[1].size, (1u64 << 32) + 5000);
            // the read stops at the end of the chain (2 sectors of 4096), the rest is zeros
            assert_eq!(got.len(), 8192);
            assert_eq!(&got[..5000], &data[..]);
            assert!(got[5000..].iter().all(|&b| b == 0));
        }
    }
}

#[test]
fn mini_cutoff_zero_means_4096() {
    let data = pattern(100, 5);
    let mut built = build(&Spec::v3(1, vec![Node::stream("A", &data)]));
    built.set_u32(0x38, 0);
    assert_eq!(read(&open(&built.bytes), "A"), data);
}

#[test]
fn fat_cycle_in_the_directory_chain_fails_open() {
    let mut built = build(&Spec::v3(1, vec![Node::stream("A", b"a")]));
    built.set_u32(built.fat_entry(built.dir_start), built.dir_start);
    assert!(is_cfb_error(CfbFile::open(&built.bytes)));
}

#[test]
fn fat_cycle_in_a_stream_fails_only_when_the_size_is_not_reached_first() {
    let data = pattern(5000, 6);
    let mut built = build(&Spec::v3(1, vec![Node::stream("Big", &data)]));
    let first = built.starts[0];
    built.set_u32(built.fat_entry(first + 9), first);
    // 10 sectors cover the 5000 bytes: the loop breaks before following the cycle (cfb.ts:212)
    assert_eq!(read(&open(&built.bytes), "Big"), data);
    // 50,000 bytes = 98 sectors, still under the guard (FAT length 128 + 2): the cycle repeats data
    built.set_u32(built.dir_entry(1) + 0x78, 50_000);
    let got = read(&open(&built.bytes), "Big");
    assert_eq!(&got[5120..5120 + 512], &data[..512]);
    // 200,000 bytes = 391 sectors: the guard trips first
    built.set_u32(built.dir_entry(1) + 0x78, 200_000);
    let file = open(&built.bytes);
    assert!(is_cfb_error(file.stream("Big")));
}

#[test]
fn mini_fat_cycle_is_detected() {
    let data = pattern(20_000, 7);
    let spec = Spec { mini_cutoff: 1 << 20, ..Spec::v3(1, vec![Node::stream("A", &data)]) };
    let mut built = build(&spec);
    assert_eq!(read(&open(&built.bytes), "A"), data);
    // one miniFAT sector (128 entries) and a self-loop: 313 mini sectors needed > 128 + 1
    built.set_u32(0x40, 1);
    built.set_u32(built.mini_fat_entry(0), 0);
    let file = open(&built.bytes);
    assert!(is_cfb_error(file.stream("A")));
    // a failed mini stream read is not fatal for the file
    assert!(is_cfb_error(file.stream("A")));
}

#[test]
fn short_mini_fat_cycle_repeats_data_without_error() {
    let data = pattern(128, 8);
    let mut built = build(&Spec::v3(1, vec![Node::stream("A", &data)]));
    built.set_u32(built.mini_fat_entry(0), 0);
    let got = read(&open(&built.bytes), "A");
    assert_eq!(&got[..64], &data[..64]);
    assert_eq!(&got[64..], &data[..64]);
}

/// A file with room for a second FAT sector, so the DIFAT walk runs (cfb.ts:104).
fn with_difat_sector(num_difat: u32) -> (Built, Vec<u8>) {
    let data = pattern(300, 10);
    let mut built = build(&Spec::v3(1, vec![Node::stream("A", &data)]));
    built.bytes.resize(built.bytes.len() + 200 * 512, 0);
    let last = (built.bytes.len() / 512 - 2) as u32;
    let off = built.sector_offset(last);
    built.bytes[off..off + 512].fill(0xff);
    built.set_u32(off + 508, last);
    built.set_u32(0x44, last);
    built.set_u32(0x48, num_difat);
    (built, data)
}

#[test]
fn difat_cycle_is_detected() {
    let (built, _) = with_difat_sector(2);
    assert!(is_cfb_error(CfbFile::open(&built.bytes)));
}

#[test]
fn difat_walk_stops_at_the_declared_count() {
    let (built, data) = with_difat_sector(1);
    assert_eq!(read(&open(&built.bytes), "A"), data);
}

#[test]
fn directory_cycles_terminate_and_list_each_entry_once() {
    // A is a storage whose child is itself; B points left to the root and right back to A
    let nodes = vec![Node::storage("A").child(1).right(2), Node::stream("B", b"b").left(0).right(1), Node::stream("Orphan", b"o")];
    let built = build(&Spec::v3(1, nodes));
    let file = open(&built.bytes);
    let paths: Vec<&str> = file.entries().iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["", "A", "B"]);
}

#[test]
fn out_of_range_ids_end_a_sibling_chain() {
    let built = build(&Spec::v3(1, vec![Node::stream("A", b"a").right(1000).left(0x8000_0000)]));
    assert_eq!(open(&built.bytes).entries().len(), 2);
}

#[test]
fn storage_depth_is_capped_at_32() {
    let mut nodes: Vec<Node> = (0..40).map(|i| Node::storage(&format!("S{i}")).child(i + 2)).collect();
    nodes.push(Node::stream("Deep", b"d"));
    let built = build(&Spec::v3(1, nodes));
    let file = open(&built.bytes);
    // S0 is at depth 0; S32 is listed but its children are not walked
    assert_eq!(file.entries().len(), 1 + 33);
    let last = file.entries().last().unwrap();
    assert_eq!(last.name, "S32");
    assert_eq!(last.path, (0..=32).map(|i| format!("S{i}")).collect::<Vec<_>>().join("/"));
    assert!(!file.has(&format!("{}/S33", last.path)));
}

#[test]
fn sectors_outside_the_file_read_as_zeros() {
    let data = pattern(5000, 11);
    let mut built = build(&Spec::v3(1, vec![Node::stream("Big", &data)]));
    built.set_u32(built.dir_entry(1) + 0x74, 1000);
    // chain [1000] (past the FAT → end): one sector, outside the file
    assert_eq!(read(&open(&built.bytes), "Big"), vec![0u8; 512]);
}

#[test]
fn truncated_file_reads_the_missing_tail_as_zeros() {
    let big = pattern(20_000, 12);
    let mut built = build(&Spec::v3(1, vec![Node::stream("Small", b"small").right(2), Node::stream("Big", &big)]));
    let len = built.bytes.len();
    built.bytes.truncate(len - 3 * 512);
    let file = open(&built.bytes);
    assert_eq!(read(&file, "Small"), b"small");
    let got = read(&file, "Big");
    assert_eq!(got.len(), 20_000);
    assert_eq!(&got[..37 * 512], &big[..37 * 512]);
    assert!(got[37 * 512..].iter().all(|&b| b == 0));
}

#[test]
fn header_lists_more_fat_sectors_than_the_file_holds() {
    let data = pattern(700, 13);
    let mut built = build(&Spec::v3(1, vec![Node::stream("A", &data)]));
    for i in 0..109 {
        built.set_u32(0x4c + i * 4, 0);
    }
    built.set_u32(0x2c, 0xffff_ffff);
    assert_eq!(read(&open(&built.bytes), "A"), data);
    // numFatSectors 0 means "as many as listed" (cfb.ts:111)
    built.set_u32(0x2c, 0);
    assert_eq!(read(&open(&built.bytes), "A"), data);
}

#[test]
fn header_errors() {
    let good = build_cfb(&[("A", b"a")]);
    assert!(CfbFile::open(&good).is_ok());
    let mut short = CFB_SIGNATURE.to_vec();
    short.resize(511, 0);
    assert!(is_cfb_error(CfbFile::open(&short)));
    assert!(is_cfb_error(CfbFile::open(b"garbage")));
    assert!(is_cfb_error(CfbFile::open(&[])));
    let patched = |off: usize, v: u16| {
        let mut b = good.clone();
        b[off..off + 2].copy_from_slice(&v.to_le_bytes());
        b
    };
    assert!(is_cfb_error(CfbFile::open(&patched(0x1c, 0xfeff))));
    assert!(is_cfb_error(CfbFile::open(&patched(0x1e, 6))));
    assert!(is_cfb_error(CfbFile::open(&patched(0x1e, 17))));
    assert!(is_cfb_error(CfbFile::open(&patched(0x20, 1))));
    assert!(is_cfb_error(CfbFile::open(&patched(0x20, 10))));
    // a valid but wrong shift reads nonsense: no root entry, no panic
    assert!(is_cfb_error(CfbFile::open(&patched(0x1e, 16))));
    let mut no_dir = good.clone();
    no_dir[0x30..0x34].copy_from_slice(&ENDC.to_le_bytes());
    assert!(is_cfb_error(CfbFile::open(&no_dir)));
    // the major version is not checked (only 3 changes the size reading)
    assert!(CfbFile::open(&patched(0x1a, 77)).is_ok());
}

/// xorshift for the mutation loop (deterministic)
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn exercise(bytes: &[u8]) {
    if let Ok(file) = CfbFile::open(bytes) {
        for e in file.entries() {
            let _ = file.stream(&e.path);
            let _ = file.has(&e.path);
        }
    }
}

fn fuzz_bases() -> Vec<Vec<u8>> {
    let nested = vec![
        Node::storage("ObjectPool").child(2).right(3),
        Node::stream("Contents", &pattern(300, 1)),
        Node::stream("WordDocument", &pattern(9000, 2)).left(4),
        Node::stream("1Table", &pattern(1500, 3)),
    ];
    vec![word_like(Spec::v3).0.bytes, word_like(Spec::v4).0.bytes, build(&Spec::v3(1, nested)).bytes]
}

#[test]
fn mutated_and_random_files_never_panic() {
    let bases = fuzz_bases();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for i in 0..3000 {
        let mut b = bases[i % bases.len()].clone();
        let flips = 1 + rng.below(8);
        for _ in 0..flips {
            // half of the flips land in the header, where the fields are
            let at = if rng.below(2) == 0 { rng.below(512) } else { rng.below(b.len()) };
            b[at] = rng.next() as u8;
        }
        if rng.below(10) == 0 {
            b.truncate(rng.below(b.len() + 1));
        }
        exercise(&b);
    }
    for _ in 0..500 {
        let len = 512 + rng.below(8192);
        let mut b: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        b[..8].copy_from_slice(&CFB_SIGNATURE);
        b[0x1c..0x1e].copy_from_slice(&0xfffeu16.to_le_bytes());
        let shift = 7 + rng.below(10) as u16;
        b[0x1e..0x20].copy_from_slice(&shift.to_le_bytes());
        b[0x20..0x22].copy_from_slice(&(2 + rng.below(shift as usize - 1) as u16).to_le_bytes());
        exercise(&b);
    }
}
