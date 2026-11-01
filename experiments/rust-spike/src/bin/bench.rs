//! Benchmark nativo: a mesma carga dos benchmarks TS (bench-ts.mjs) e C# (FontBench):
//! parse das 26 fontes PARSE_ITERS vezes; medição do texto com cada fonte MEASURE_ITERS
//! vezes; paridade por fonte. Saída: JSON com as mesmas chaves dos outros.
use std::time::Instant;
use uofonts::{measure, sfnt::SfntFont};

const PARSE_ITERS: u32 = 10;
const MEASURE_ITERS: u32 = 20;
const SIZE_PT: f64 = 11.0;

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    let names: Vec<String> = std::fs::read_to_string(dir.join("fonts.txt")).unwrap().lines().filter(|l| !l.is_empty()).map(String::from).collect();
    let bytes: Vec<Vec<u8>> = names.iter().map(|n| std::fs::read(dir.join("fonts").join(n)).unwrap()).collect();
    let text: Vec<u32> = std::fs::read_to_string(dir.join("text.txt")).unwrap().chars().map(|c| c as u32).collect();

    // aquecimento (o mesmo nos três: uma passada de cada fase antes de medir)
    let mut fonts: Vec<SfntFont> = bytes.iter().map(|b| SfntFont::new(b.clone()).unwrap()).collect();
    for f in &fonts {
        measure(f, &text, SIZE_PT);
    }

    let t = Instant::now();
    for _ in 0..PARSE_ITERS {
        fonts = bytes.iter().map(|b| SfntFont::new(b.clone()).unwrap()).collect();
    }
    let parse_ms = t.elapsed().as_secs_f64() * 1000.0 / PARSE_ITERS as f64;

    let t = Instant::now();
    let mut total = 0.0;
    for i in 0..MEASURE_ITERS {
        let mut s = 0.0;
        for f in &fonts {
            s += measure(f, &text, SIZE_PT);
        }
        if i == 0 {
            total = s;
        }
    }
    let measure_ms = t.elapsed().as_secs_f64() * 1000.0 / MEASURE_ITERS as f64;

    let parity: Vec<String> = fonts
        .iter()
        .zip(&names)
        .map(|(f, n)| {
            format!(
                "\"{}\":[{},{},{},{},{:.6}]",
                n,
                f.kern_pairs(),
                f.kern_class0_len(),
                if f.kern_in_latin { 1 } else { 0 },
                f.metrics.units_per_em,
                measure(f, &text, SIZE_PT)
            )
        })
        .collect();
    println!(
        "{{\"impl\":\"rust-native\",\"parseMs\":{:.2},\"measureMs\":{:.2},\"glyphs\":{},\"total\":{:.6},\"parity\":{{{}}}}}",
        parse_ms,
        measure_ms,
        text.len() * fonts.len(),
        total,
        parity.join(",")
    );
}
