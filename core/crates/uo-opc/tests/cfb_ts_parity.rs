//! Differential check against the frozen TS readers (docx `doc/cfb.ts`, sheet `xls/cfb.ts`,
//! slides `ppt/cfb.ts`), run by Node (type stripping) on generated and mutated files. Opt-in:
//! `UO_TS_PARITY=1 cargo test -p uo-opc --test cfb_ts_parity` (needs `node` ≥ 22.18 on PATH).
//! Compared per file: open error or not; per entry the name and path (UTF-8, so a lone
//! surrogate is U+FFFD on both sides), kind, size (as the JS number), start, `has`, and each
//! stream read (length + FNV-1a, `-` for not a stream, `E` for an error).

mod cfb_support;

use std::path::{Path, PathBuf};
use std::process::Command;

use cfb_support::{Node, Spec, build, pattern, root_streams};
use uo_opc::cfb::{CFB_SIGNATURE, CfbEntryKind, CfbFile};

const NODE_SCRIPT: &str = r#"
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
const [, , modPath, dir, count] = process.argv;
const { readCfb } = await import(pathToFileURL(modPath).href);
const enc = new TextEncoder();
const hex = (s) => Buffer.from(enc.encode(s)).toString('hex');
const fnv = (u8) => { let h = 0x811c9dc5; for (const b of u8) { h ^= b; h = Math.imul(h, 0x01000193) >>> 0; } return h; };
const out = [];
for (let i = 0; i < Number(count); i++) {
  const bytes = new Uint8Array(readFileSync(`${dir}/${i}.cfb`));
  let line;
  try {
    const f = readCfb(bytes);
    const parts = f.entries.map((e) => {
      let s;
      try { const d = f.getStream(e.path); s = d === undefined ? '-' : `${d.length}:${fnv(d)}`; } catch { s = 'E'; }
      return [hex(e.name), hex(e.path), e.type, BigInt(e.size).toString(), e.start, f.has(e.path) ? 1 : 0, s].join(',');
    });
    line = 'ok;' + parts.join(';');
  } catch { line = 'err'; }
  out.push(line);
}
process.stdout.write(out.join('\n') + '\n');
"#;

fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn fnv(d: &[u8]) -> u32 {
    d.iter().fold(0x811c_9dc5u32, |h, &b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
}

fn describe(bytes: &[u8]) -> String {
    let Ok(f) = CfbFile::open(bytes) else { return "err".into() };
    let parts: Vec<String> = f
        .entries()
        .iter()
        .map(|e| {
            let s = match f.stream(&e.path) {
                Ok(Some(d)) => format!("{}:{}", d.len(), fnv(&d)),
                Ok(None) => "-".into(),
                Err(_) => "E".into(),
            };
            let kind = match e.kind {
                CfbEntryKind::Root => "root",
                CfbEntryKind::Storage => "storage",
                CfbEntryKind::Stream => "stream",
            };
            // the JS number: round to f64, then print the integer exactly
            let size = (e.size as f64) as u128;
            format!("{},{},{kind},{size},{},{},{s}", hex(&e.name), hex(&e.path), e.start, u8::from(f.has(&e.path)))
        })
        .collect();
    format!("ok;{}", parts.join(";"))
}

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

fn corpus() -> Vec<Vec<u8>> {
    let small = pattern(1000, 1);
    let big = pattern(9000, 2);
    let edge = pattern(4095, 3);
    let streams: [(&str, &[u8]); 4] = [("WordDocument", &big), ("1Table", &small), ("\u{5}SummaryInformation", &edge), ("Empty", &[])];
    let (child, nodes) = root_streams(&streams);
    let nested = vec![
        Node::storage("ObjectPool").child(2).right(3),
        Node::stream("Contents", &small).right(5),
        Node::stream("ÉTÉ Σ", &big).left(4),
        Node::storage("Inner"),
        Node::stream("Dup", &edge).right(6),
        Node::stream("dup", &small),
    ];
    let deep: Vec<Node> = (0..36).map(|i| Node::storage(&format!("S{i}")).child(i + 2)).chain([Node::stream("Deep", b"d")]).collect();
    let mut bases = vec![
        build(&Spec::v3(child, nodes.clone())),
        build(&Spec::v4(child, nodes)),
        build(&Spec::v3(1, nested)),
        build(&Spec::v3(1, deep)),
        build(&Spec { mini_cutoff: 1 << 20, ..Spec::v3(1, vec![Node::stream("A", &pattern(20_000, 4))]) }),
    ];
    // a few targeted quirks on top of the bases
    let mut quirks = Vec::new();
    let mut q = bases[0].clone();
    q.set_u32(q.dir_entry(1) + 0x7c, 9);
    quirks.push(q.bytes.clone());
    let mut q = bases[1].clone();
    q.set_u32(q.dir_entry(1) + 0x7c, 0xffff_ffff);
    q.set_u32(q.dir_entry(1) + 0x78, 0xffff_ffff);
    quirks.push(q.bytes.clone());
    let mut q = bases[4].clone();
    q.set_u32(0x40, 1);
    q.set_u32(q.mini_fat_entry(0), 0);
    quirks.push(q.bytes.clone());
    let mut q = bases[0].clone();
    let first = q.starts[0];
    q.set_u32(q.fat_entry(first + 1), first);
    quirks.push(q.bytes.clone());
    q.set_u32(q.dir_entry(1) + 0x78, 300_000);
    quirks.push(q.bytes.clone());
    let mut q = bases[0].clone();
    q.bytes.truncate(q.bytes.len() - 2000);
    quirks.push(q.bytes.clone());
    let mut q = bases[2].clone();
    q.set_u16(q.dir_entry(3) + 0x40, 7);
    q.set_u16(q.dir_entry(3), 0xd800);
    quirks.push(q.bytes.clone());
    let mut q = bases[0].clone();
    q.set_u32(0x38, 0);
    quirks.push(q.bytes.clone());

    let base_bytes: Vec<Vec<u8>> = bases.drain(..).map(|b| b.bytes).collect();
    let mut all = base_bytes.clone();
    all.extend(quirks);
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for i in 0..600 {
        let mut b = base_bytes[i % base_bytes.len()].clone();
        for _ in 0..1 + rng.below(6) {
            let at = if rng.below(2) == 0 { rng.below(512) } else { rng.below(b.len()) };
            b[at] = rng.next() as u8;
        }
        if rng.below(10) == 0 {
            b.truncate(rng.below(b.len() + 1));
        }
        all.push(b);
    }
    for _ in 0..100 {
        let len = 512 + rng.below(6000);
        let mut b: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        b[..8].copy_from_slice(&CFB_SIGNATURE);
        b[0x1c..0x1e].copy_from_slice(&0xfffeu16.to_le_bytes());
        let shift = 7 + rng.below(10) as u16;
        b[0x1e..0x20].copy_from_slice(&shift.to_le_bytes());
        b[0x20..0x22].copy_from_slice(&(2 + rng.below(shift as usize - 1) as u16).to_le_bytes());
        b[0x30..0x34].copy_from_slice(&(rng.below(8) as u32).to_le_bytes());
        all.push(b);
    }
    all
}

fn ts_sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    ["write/packages/docx-core/src/doc/cfb.ts", "sheet/packages/sheet-core/src/xls/cfb.ts", "slides/packages/slides-core/src/ppt/cfb.ts"]
        .iter()
        .map(|p| root.join(p))
        .filter(|p| p.exists())
        .collect()
}

#[test]
fn matches_the_ts_readers() {
    if std::env::var("UO_TS_PARITY").as_deref() != Ok("1") {
        eprintln!("skipped: set UO_TS_PARITY=1 to compare with the TS readers through node");
        return;
    }
    let files = corpus();
    let dir = std::env::temp_dir().join(format!("uo-cfb-parity-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (i, b) in files.iter().enumerate() {
        std::fs::write(dir.join(format!("{i}.cfb")), b).unwrap();
    }
    let script = dir.join("cfb-describe.mjs");
    std::fs::write(&script, NODE_SCRIPT).unwrap();
    let expected: Vec<String> = files.iter().map(|b| describe(b)).collect();
    let sources = ts_sources();
    assert!(!sources.is_empty(), "no TS reader found");
    for src in &sources {
        let out = Command::new("node").arg(&script).arg(src).arg(&dir).arg(files.len().to_string()).output().unwrap();
        assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
        let stdout = String::from_utf8(out.stdout).unwrap();
        let got: Vec<&str> = stdout.lines().collect();
        assert_eq!(got.len(), files.len());
        let mismatches: Vec<usize> = (0..files.len()).filter(|&i| got[i] != expected[i]).collect();
        for &i in mismatches.iter().take(3) {
            eprintln!("file {i}\n  ts:   {}\n  rust: {}", got[i], expected[i]);
        }
        assert!(mismatches.is_empty(), "{} of {} files differ from {}", mismatches.len(), files.len(), src.display());
        eprintln!("{}: {} files identical", src.display(), files.len());
    }
    let opened = expected.iter().filter(|l| l.starts_with("ok")).count();
    eprintln!("{opened} opened, {} rejected", files.len() - opened);
    let _ = std::fs::remove_dir_all(&dir);
}
