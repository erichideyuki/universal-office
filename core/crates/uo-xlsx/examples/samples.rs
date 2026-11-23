//! Writes the sample workbooks of `tests/support/samples.rs` into a folder, for the Excel and
//! LibreOffice open check (`tools/open-check.ps1`):
//!
//!   cargo run --release -p uo-xlsx --example samples -- <out dir>
#![allow(clippy::unwrap_used)]

#[path = "../tests/support/samples.rs"]
mod samples;

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "target/xlsx-samples".into());
    std::fs::create_dir_all(&dir).unwrap();
    let out = |name: &str, bytes: Vec<u8>| {
        let p = std::path::Path::new(&dir).join(name);
        println!("{} {} bytes", p.display(), bytes.len());
        std::fs::write(p, bytes).unwrap();
    };
    out("minimal.xlsx", samples::minimal());
    out("features.xlsx", samples::features(true));
    out("features-inline.xlsx", samples::features(false));
    let t = std::time::Instant::now();
    let h = samples::heatmap(10_000, 40, true);
    println!("heatmap 10,000 x 40: {:?}", t.elapsed());
    out("heatmap-10k.xlsx", h);
    out("heatmap-inline-1k.xlsx", samples::heatmap(1_000, 40, false));
    // negative controls (must FAIL the open check, proving it detects a repair): a shared-string
    // index past the table, and overlapping merged ranges
    let controls = std::path::Path::new(&dir).join("controls");
    std::fs::create_dir_all(&controls).unwrap();
    let edit = |src: &[u8], part: &str, from: &str, to: &str| -> Vec<u8> {
        let entries = uo_opc::zip::read(src, 100).unwrap();
        let files: Vec<(String, Vec<u8>)> = entries
            .iter()
            .map(|e| {
                let raw = e.compressed(src);
                let d = if e.method == 0 { raw.to_vec() } else { uo_opc::flate::inflate_raw(raw, Some(e.size), 1 << 30).unwrap() };
                let d = if e.name == part { String::from_utf8(d).unwrap().replacen(from, to, 1).into_bytes() } else { d };
                (e.name.clone(), d)
            })
            .collect();
        uo_opc::zip::write(files.iter().map(|(n, d)| (n.as_str(), d.as_slice())), uo_opc::zip::WriteOptions::default()).unwrap()
    };
    let m = samples::minimal();
    std::fs::write(controls.join("bad-sst-index.xlsx"), edit(&m, "xl/worksheets/sheet1.xml", "<v>0</v>", "<v>9</v>")).unwrap();
    std::fs::write(
        controls.join("overlapping-merges.xlsx"),
        edit(&m, "xl/worksheets/sheet1.xml", "</sheetData>", "</sheetData><mergeCells count=\"2\"><mergeCell ref=\"A1:B2\"/><mergeCell ref=\"B2:C3\"/></mergeCells>"),
    )
    .unwrap();
}
