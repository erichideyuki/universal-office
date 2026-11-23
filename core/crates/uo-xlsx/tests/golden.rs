//! Golden files: every part of the `features` sample (the one checked in Excel by
//! `tools/excel-open-check.ps1`, record "Excel open check") is compared byte for byte with
//! `tests/golden/features/`. A change of the output is a deliberate act: rerun the open check,
//! then `UO_XLSX_BLESS=1 cargo test -p uo-xlsx --test golden`.
#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use std::path::PathBuf;

fn parts(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    uo_opc::zip::read(bytes, 1000)
        .unwrap()
        .iter()
        .map(|e| {
            let raw = e.compressed(bytes);
            let d = if e.method == 0 { raw.to_vec() } else { uo_opc::flate::inflate_raw(raw, Some(e.size), 1 << 30).unwrap() };
            (e.name.clone(), d)
        })
        .collect()
}

#[test]
fn features_parts_equal_golden() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/features");
    let bless = std::env::var_os("UO_XLSX_BLESS").is_some();
    let got = parts(&support::samples::features(true));
    let names: Vec<String> = got.iter().map(|(n, _)| n.replace('/', "__")).collect();
    if bless {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for ((_, d), n) in got.iter().zip(&names) {
            std::fs::write(dir.join(n), d).unwrap();
        }
        std::fs::write(dir.join("ORDER.txt"), names.join("\n") + "\n").unwrap();
        return;
    }
    let order = std::fs::read_to_string(dir.join("ORDER.txt")).unwrap();
    assert_eq!(order.lines().collect::<Vec<_>>(), names, "entry order");
    for ((name, d), n) in got.iter().zip(&names) {
        let want = std::fs::read(dir.join(n)).unwrap();
        assert!(want == *d, "{name} differs from tests/golden/features/{n}:\n{}", String::from_utf8_lossy(d));
    }
}
