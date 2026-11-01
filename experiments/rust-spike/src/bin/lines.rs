//! Spike 2 nativo: a réplica da quebra de linha (data/lines.bin) com as mesmas métricas das
//! réplicas TS e C#: tempo por passada (parede e CPU do processo), memória alocada numa
//! passada, memória retida pelas linhas produzidas e pico do processo (working set).
use std::time::Instant;
use uofonts::{linebreak, linedata, memcount};

#[repr(C)]
#[derive(Default)]
struct Pmc {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct FileTime(u32, u32);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32GetProcessMemoryInfo(h: isize, p: *mut Pmc, cb: u32) -> i32;
    fn GetProcessTimes(h: isize, c: *mut FileTime, e: *mut FileTime, k: *mut FileTime, u: *mut FileTime) -> i32;
}

fn peak_working_set_mb() -> f64 {
    let mut p = Pmc { cb: std::mem::size_of::<Pmc>() as u32, ..Default::default() };
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut p, p.cb) };
    p.peak_working_set_size as f64 / 1048576.0
}

fn cpu_ms() -> f64 {
    let (mut c, mut e, mut k, mut u) = (FileTime::default(), FileTime::default(), FileTime::default(), FileTime::default());
    unsafe { GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) };
    let t = |f: FileTime| ((f.1 as u64) << 32 | f.0 as u64) as f64 / 10_000.0;
    t(k) + t(u)
}

fn main() {
    let iters: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(20);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data").join("lines.bin");
    let corpus = linedata::read_corpus(&std::fs::read(path).unwrap());
    let glyphs: usize = corpus.paras.iter().map(|p| p.glyphs.len()).sum();

    let out = linebreak::replay(&corpus);
    let bad = linedata::mismatches(&out, &corpus.expected);
    drop(out);
    linebreak::replay(&corpus);

    let mut times = Vec::with_capacity(iters);
    let cpu0 = cpu_ms();
    for _ in 0..iters {
        let t = Instant::now();
        std::hint::black_box(linebreak::replay(&corpus));
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let cpu = (cpu_ms() - cpu0) / iters as f64;
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());

    // memória de uma passada: alocado, pico acima do que já estava vivo e o que fica nas linhas
    let (t0, live0, _) = memcount::stats();
    memcount::reset_peak();
    let kept = linebreak::replay(&corpus);
    let (t1, live1, peak) = memcount::stats();
    drop(kept);
    let mb = |b: usize| b as f64 / 1048576.0;
    println!(
        "{{\"impl\":\"rust-native\",\"lines\":{},\"glyphs\":{},\"mismatches\":{},\"bestMs\":{:.2},\"medianMs\":{:.2},\"cpuMsPerIter\":{:.2},\"allocatedMB\":{:.2},\"peakAboveBaseMB\":{:.2},\"retainedMB\":{:.2},\"peakRssMB\":{:.2}}}",
        corpus.expected.len(),
        glyphs,
        bad,
        times[0],
        times[times.len() / 2],
        cpu,
        mb(t1 - t0),
        mb(peak - live0),
        mb(live1 - live0),
        peak_working_set_mb()
    );
}
