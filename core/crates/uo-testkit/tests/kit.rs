//! The kit itself: SHA-256 vectors, the stream encoding pinned to the digest `js/testkit.mjs`
//! computes for the same items, the PRNG sequence of the JS generators, the capture format and
//! the replay bookkeeping, and the zip builder read back by the independent listing.

use uo_testkit::capture::Capture;
use uo_testkit::generate::{RawZipEntry, RawZipOptions, Rng, build_zip};
use uo_testkit::sha256::{Sha256, sha256};
use uo_testkit::stream::hex;
use uo_testkit::zipdir::central_directory;
use uo_testkit::{Replay, Stream};

#[test]
fn sha256_vectors() {
    assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(
        hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    let million = vec![b'a'; 1_000_000];
    assert_eq!(hex(&sha256(&million)), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    // split updates at every awkward boundary give the same digest
    for split in [1, 55, 56, 63, 64, 65, 127, 129] {
        let mut h = Sha256::new();
        for c in million[..10_000].chunks(split) {
            h.update(c);
        }
        assert_eq!(h.finish(), sha256(&million[..10_000]), "split {split}");
    }
}

#[test]
fn stream_matches_the_js_encoder() {
    // the same items written by js/testkit.mjs `Stream` (the JS string had a lone surrogate
    // where this one has U+FFFD: both encode it as EF BF BD)
    let mut s = Stream::traced();
    s.tag(b'E').u32(7).u64(1 << 40).i64(-5).f64(-0.0).f64(1.5).bool(true).str("é😀\u{FFFD}\n\"\u{1}").bytes(&[1, 2, 3]).digest_of(b"abc");
    s.str(&"x".repeat(70_000));
    let lines: Vec<&str> = s.trace().lines().take(10).collect();
    assert_eq!(
        lines,
        [
            "tag E",
            "u32 7",
            "u64 1099511627776",
            "i64 -5",
            "f64 8000000000000000",
            "f64 3ff8000000000000",
            "bool 1",
            "str \"é😀\u{FFFD}\\n\\\"\\u0001\"",
            "bytes 3 010203",
            "digest 3 ba7816bf8f01cfea414140de5dae2223",
        ]
    );
    assert_eq!(s.items(), 11);
    assert_eq!(s.digest().hex(), "ef7e1be9c51ccc0668ed65435e2e92e633ac22c23913374fb8dc5718bb9a0d92");
    assert_eq!(s.finish().short(), "ef7e1be9c51ccc0668ed65435e2e92e6");
}

#[test]
fn rng_is_the_js_xorshift() {
    let mut r = Rng::new(1);
    assert_eq!([r.next_u32(), r.next_u32()], [270_369, 67_634_689]);
    assert_eq!(r.below(10), 1);
}

#[test]
fn capture_and_replay() {
    let text = "# header\r\n@ts abc123\n\ndoc/a\tload=ok:2:ff\txml=1:0:aa\ndoc/b\tload=err:main\n";
    let c = Capture::parse(text).unwrap();
    assert_eq!(c.meta("ts"), Some("abc123"));
    assert_eq!(c.len(), 2);
    assert_eq!(c.get("doc/a", "xml"), Some("1:0:aa"));
    assert!(Capture::parse("k\tnovalue\n").is_err());
    assert!(Capture::parse("k\ta=1\nk\tb=2\n").is_err());

    let mut r = Replay::new(&c);
    assert!(r.check("doc/a", "load", "ok:2:ff"));
    assert!(!r.check("doc/a", "xml", "1:0:bb"));
    assert!(!r.accept("doc/b", "load", "err:zip", "decision X"));
    assert!(!r.check("doc/c", "load", "ok:0:00"));
    let report = r.finish();
    assert_eq!(report.equal, 1);
    assert_eq!(report.mismatches.len(), 2);
    assert_eq!(report.mismatches[1].expected, None);
    assert_eq!(report.accepted.len(), 1);
    assert!(report.missing.is_empty());
    assert!(!report.is_clean());
    assert!(report.summary().contains("accepted 1× decision X"));

    let mut r = Replay::new(&c);
    r.check("doc/a", "load", "ok:2:ff");
    let report = r.finish();
    assert_eq!(report.missing.len(), 2, "unreplayed facets are reported");
    assert!(std::panic::catch_unwind(|| report.assert_clean()).is_err());
}

#[test]
fn zip_builder_and_listing() {
    let entries = [
        RawZipEntry::stored("a.xml", b"<a/>".to_vec()),
        RawZipEntry::stored([0xc3, 0xa9].to_vec(), b"x".to_vec()).flags(0x800),
        RawZipEntry::deflated("d", vec![3, 0], 0),
    ];
    let z = build_zip(&entries, &RawZipOptions::default());
    let list = central_directory(&z).unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].name, b"a.xml");
    assert_eq!(list[1].flags, 0x800);
    assert_eq!(list[2].method, 8);
    assert_eq!(list[0].local_offset, 0);
    let z64 = build_zip(&entries[..1], &RawZipOptions { zip64: true, ..RawZipOptions::default() });
    assert_eq!(central_directory(&z64).unwrap().len(), 1);
    assert!(central_directory(&z[..10]).is_err());
}
