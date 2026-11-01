//! M1 for the CFB reader: open + read every stream, on valid and invalid files, returns live
//! bytes exactly to where they were (the lazily read mini stream included).

mod cfb_support;

use cfb_support::{Node, Spec, build, pattern, root_streams};
use uo_alloc::assert_no_leak;
use uo_opc::cfb::CfbFile;

fn open_and_read_all(bytes: &[u8]) -> usize {
    let Ok(file) = CfbFile::open(bytes) else { return 0 };
    let mut total = 0;
    for e in file.entries() {
        // twice: the second read uses the cached mini stream
        for _ in 0..2 {
            if let Ok(Some(s)) = file.stream(&e.path) {
                total += s.len();
            }
        }
        let _ = file.has(&e.path);
    }
    total
}

fn check(bytes: &[u8]) {
    // warm-up: first-use allocations of the runtime (hash seeds) are not the reader's
    open_and_read_all(bytes);
    assert_no_leak(|| open_and_read_all(bytes));
}

fn valid_files() -> Vec<Vec<u8>> {
    let small = pattern(1000, 1);
    let big = pattern(9000, 2);
    let streams: [(&str, &[u8]); 3] = [("WordDocument", &big), ("1Table", &small), ("Empty", &[])];
    let (child, nodes) = root_streams(&streams);
    let nested = vec![Node::storage("ObjectPool").child(2).right(3), Node::stream("Contents", &small), Node::stream("Data", &big)];
    vec![build(&Spec::v3(child, nodes.clone())).bytes, build(&Spec::v4(child, nodes)).bytes, build(&Spec::v3(1, nested)).bytes]
}

#[test]
fn m1_valid_files() {
    for bytes in valid_files() {
        check(&bytes);
        let usage = assert_no_leak(|| open_and_read_all(&bytes));
        assert!(usage.allocated > 0);
    }
}

#[test]
fn m1_open_errors() {
    let good = valid_files().remove(0);
    let mut dir_cycle = build(&Spec::v3(1, vec![Node::stream("A", b"a")]));
    dir_cycle.set_u32(dir_cycle.fat_entry(dir_cycle.dir_start), dir_cycle.dir_start);
    let mut bad_shift = good.clone();
    bad_shift[0x1e] = 16;
    let mut truncated = good.clone();
    truncated.truncate(300);
    for bytes in [dir_cycle.bytes, bad_shift, truncated, b"garbage".to_vec()] {
        check(&bytes);
    }
}

#[test]
fn m1_stream_errors() {
    // miniFAT cycle: the mini stream is cached, then the read fails
    let data = pattern(20_000, 3);
    let spec = Spec { mini_cutoff: 1 << 20, ..Spec::v3(1, vec![Node::stream("A", &data)]) };
    let mut mini_cycle = build(&spec);
    mini_cycle.set_u32(0x40, 1);
    mini_cycle.set_u32(mini_cycle.mini_fat_entry(0), 0);
    assert!(CfbFile::open(&mini_cycle.bytes).unwrap().stream("A").is_err());
    check(&mini_cycle.bytes);
    // FAT cycle in a regular stream
    let mut fat_cycle = build(&Spec::v3(1, vec![Node::stream("Big", &pattern(5000, 4))]));
    let first = fat_cycle.starts[0];
    fat_cycle.set_u32(fat_cycle.fat_entry(first + 9), first);
    fat_cycle.set_u32(fat_cycle.dir_entry(1) + 0x78, 200_000);
    assert!(CfbFile::open(&fat_cycle.bytes).unwrap().stream("Big").is_err());
    check(&fat_cycle.bytes);
    // a failing mini stream (FAT cycle in the root chain) for every small stream
    let mut root_cycle = build(&Spec::v3(1, vec![Node::stream("S", &pattern(100, 5))]));
    let ms = root_cycle.mini_stream_start;
    root_cycle.set_u32(root_cycle.fat_entry(ms), ms);
    root_cycle.set_u32(0x38, 1 << 20);
    root_cycle.set_u32(root_cycle.dir_entry(0) + 0x78, 1 << 20);
    assert!(CfbFile::open(&root_cycle.bytes).unwrap().stream("S").is_err());
    check(&root_cycle.bytes);
}
