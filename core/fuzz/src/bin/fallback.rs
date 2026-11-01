//! Stable-Rust fuzz driver: runs the targets of `uo_opc_fuzz::TARGETS` over a seeded mutation
//! loop (no coverage feedback) starting from `corpus/<target>/`, for machines without
//! cargo-fuzz/libFuzzer. A panic is a crash: the input goes to `artifacts/<target>/crash-<hash>`
//! and the run fails at the end; an execution slower than `--slow` seconds goes to
//! `artifacts/<target>/slow-<hash>`.
//!
//!   cargo run --release --no-default-features --features fallback --bin fallback -- \
//!       [--secs N | --iters N] [--seed N] [--max-len N] [--slow S] [target...]
//!   … --bin fallback -- --reproduce <target> <file>
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::time::{Duration, Instant};

thread_local! {
    static LAST_PANIC: RefCell<String> = const { RefCell::new(String::new()) };
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

const INTERESTING: &[u32] =
    &[0, 1, 0x7F, 0x80, 0xFF, 0x100, 0x7FFF, 0x8000, 0xFFFF, 0x1_0000, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF_FFFE, 0xFFFF_FFFF];
const TOKENS: &[&[u8]] = &[
    b"PK\x03\x04",
    b"PK\x01\x02",
    b"PK\x05\x06",
    b"PK\x06\x06",
    b"PK\x06\x07",
    b"<",
    b">",
    b"/>",
    b"</",
    b"<!--",
    b"-->",
    b"<![CDATA[",
    b"]]>",
    b"<?",
    b"?>",
    b"&amp;",
    b"&#x",
    b"&#",
    b";",
    b"xmlns:",
    b"xmlns=\"",
    b"=\"",
    b"\"",
    b"'",
    b"%2e%2e/",
    b"../",
    b"%",
    b"\\",
    b"#",
    b"<!DOCTYPE",
    b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1",
    b"\xff\xfe",
    b"\xfe\xff",
    b"\xef\xbb\xbf",
    b"\0",
    b"\r\n",
];

fn mutate(rng: &mut Rng, input: &mut Vec<u8>, pool: &[Vec<u8>], max_len: usize) {
    let rounds = 1 + rng.below(4);
    for _ in 0..rounds {
        let len = input.len();
        match rng.below(11) {
            0 if len > 0 => {
                let i = rng.below(len);
                input[i] ^= 1 << rng.below(8);
            }
            1 if len > 0 => {
                let i = rng.below(len);
                input[i] = rng.next() as u8;
            }
            2 if len > 0 => {
                let v = INTERESTING[rng.below(INTERESTING.len())].to_le_bytes();
                let w = [1, 2, 4][rng.below(3)];
                let i = rng.below(len);
                for (k, b) in v.iter().take(w).enumerate() {
                    if let Some(x) = input.get_mut(i + k) {
                        *x = *b;
                    }
                }
            }
            3 => {
                let i = rng.below(len + 1);
                let n = 1 + rng.below(16);
                let bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                input.splice(i..i, bytes);
            }
            4 if len > 0 => {
                let i = rng.below(len);
                let n = 1 + rng.below((len - i).min(64));
                input.drain(i..i + n);
            }
            5 if len > 0 => {
                // duplicate a chunk somewhere (repetition finds quadratic paths)
                let i = rng.below(len);
                let n = 1 + rng.below((len - i).min(256));
                let chunk = input[i..i + n].to_vec();
                let most = if rng.below(8) == 0 { 256 } else { 4 };
                let reps = 1 + rng.below(most);
                let at = rng.below(len + 1);
                input.splice(at..at, chunk.iter().copied().cycle().take(n * reps));
            }
            6 if !pool.is_empty() => {
                // splice with another input
                let other = &pool[rng.below(pool.len())];
                if !other.is_empty() {
                    let a = rng.below(other.len());
                    let n = 1 + rng.below((other.len() - a).min(1024));
                    let at = rng.below(len + 1);
                    let cut = rng.below(len - at + 1).min(n);
                    input.splice(at..at + cut, other[a..a + n].iter().copied());
                }
            }
            7 => {
                let t = TOKENS[rng.below(TOKENS.len())];
                let at = rng.below(len + 1);
                if rng.below(2) == 0 {
                    input.splice(at..at, t.iter().copied());
                } else {
                    let end = (at + t.len()).min(len);
                    input.splice(at..end, t.iter().copied());
                }
            }
            8 if len > 1 => {
                input.truncate(rng.below(len));
            }
            9 if len > 0 => {
                // the selector byte: try every preset/mode
                input[0] = rng.next() as u8;
            }
            _ if len > 0 => {
                // small arithmetic on a byte
                let i = rng.below(len);
                input[i] = input[i].wrapping_add((rng.below(35) as u8).wrapping_sub(17));
            }
            _ => input.push(rng.next() as u8),
        }
    }
    input.truncate(max_len);
}

fn load(dir: &PathBuf) -> Vec<Vec<u8>> {
    let mut v: Vec<(String, Vec<u8>)> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_file())
                .map(|e| (e.file_name().to_string_lossy().into_owned(), fs::read(e.path()).unwrap()))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    let mut v: Vec<Vec<u8>> = v.into_iter().map(|(_, d)| d).collect();
    if v.is_empty() {
        v.push(Vec::new());
    }
    v
}

fn fnv(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

fn run_one(f: uo_opc_fuzz::Target, data: &[u8]) -> (Result<(), String>, Duration) {
    let t = Instant::now();
    let r = panic::catch_unwind(AssertUnwindSafe(|| f(data)));
    let took = t.elapsed();
    (r.map_err(|_| LAST_PANIC.with(|p| p.borrow().clone())), took)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fuzz = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    panic::set_hook(Box::new(|info| {
        let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let msg = info
            .payload()
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| info.payload().downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_default();
        LAST_PANIC.with(|p| *p.borrow_mut() = format!("{msg} at {loc}"));
    }));

    if args.first().map(String::as_str) == Some("--reproduce") {
        let (Some(name), Some(file)) = (args.get(1), args.get(2)) else { panic!("--reproduce <target> <file>") };
        let f = uo_opc_fuzz::TARGETS.iter().find(|(n, _)| n == name).expect("unknown target").1;
        let data = fs::read(file).unwrap();
        let (r, took) = run_one(f, &data);
        println!("{name} {file}: {r:?} in {took:?}");
        std::process::exit(i32::from(r.is_err()));
    }

    let mut secs: Option<u64> = None;
    let mut iters: Option<u64> = None;
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut max_len = 64 * 1024;
    let mut slow = 2.0f64;
    let mut names: Vec<String> = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--secs" => secs = it.next().and_then(|v| v.parse().ok()),
            "--iters" => iters = it.next().and_then(|v| v.parse().ok()),
            "--seed" => seed = it.next().and_then(|v| v.parse().ok()).unwrap_or(seed),
            "--max-len" => max_len = it.next().and_then(|v| v.parse().ok()).unwrap_or(max_len),
            "--slow" => slow = it.next().and_then(|v| v.parse().ok()).unwrap_or(slow),
            _ => names.push(a),
        }
    }
    if secs.is_none() && iters.is_none() {
        secs = Some(10);
    }
    let targets: Vec<(&str, uo_opc_fuzz::Target)> =
        uo_opc_fuzz::TARGETS.iter().copied().filter(|(n, _)| names.is_empty() || names.iter().any(|x| x == n)).collect();
    let mut failed = false;
    println!("{:<18} {:>10} {:>8} {:>10} {:>6} {:>6} {:>10}", "target", "execs", "secs", "execs/s", "seeds", "crash", "slowest");
    for (name, f) in targets {
        let mut pool = load(&fuzz.join("corpus").join(name));
        let seeds = pool.len();
        let artifacts = fuzz.join("artifacts").join(name);
        let mut rng = Rng(seed ^ fnv(name.as_bytes()) | 1);
        let start = Instant::now();
        let mut execs = 0u64;
        let mut crashes = 0;
        let mut slowest = Duration::ZERO;
        // the seeds themselves first
        let mut queue: Vec<Vec<u8>> = pool.clone();
        loop {
            if secs.is_some_and(|s| start.elapsed() >= Duration::from_secs(s)) || iters.is_some_and(|n| execs >= n) {
                break;
            }
            let input = match queue.pop() {
                Some(q) => q,
                None => {
                    let mut x = pool[rng.below(pool.len())].clone();
                    mutate(&mut rng, &mut x, &pool, max_len);
                    x
                }
            };
            let (r, took) = run_one(f, &input);
            execs += 1;
            slowest = slowest.max(took);
            if let Err(msg) = r {
                crashes += 1;
                failed = true;
                fs::create_dir_all(&artifacts).unwrap();
                let path = artifacts.join(format!("crash-{:016x}", fnv(&input)));
                if crashes <= 5 {
                    println!("CRASH {name}: {msg}\n  input saved to {}", path.display());
                }
                fs::write(path, &input).unwrap();
            } else if took.as_secs_f64() > slow {
                fs::create_dir_all(&artifacts).unwrap();
                let path = artifacts.join(format!("slow-{:016x}", fnv(&input)));
                println!("SLOW {name}: {took:?}, input saved to {}", path.display());
                fs::write(path, &input).unwrap();
                failed = true;
            }
            // a random walk: some mutants become parents (no coverage feedback here)
            if rng.below(32) == 0 && input.len() <= max_len {
                if pool.len() < 4 * seeds + 256 {
                    pool.push(input);
                } else {
                    let i = seeds + rng.below(pool.len() - seeds).min(pool.len() - seeds - 1);
                    pool[i] = input;
                }
            }
        }
        let el = start.elapsed().as_secs_f64();
        println!(
            "{:<18} {:>10} {:>8.1} {:>10.0} {:>6} {:>6} {:>10.3?}",
            name,
            execs,
            el,
            execs as f64 / el.max(1e-9),
            seeds,
            crashes,
            slowest
        );
    }
    if failed {
        std::process::exit(1);
    }
}
