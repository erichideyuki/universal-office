//! Converts a `.doc` (Word 97-2003) into a `.docx`, or unpacks the parts of the result, with the
//! converter of `uo_docx::doc` (feature `doc`):
//!
//! ```text
//! cargo run --release -p uo-docx --features doc --example doc_to_docx -- in.doc out.docx
//! cargo run --release -p uo-docx --features doc --example doc_to_docx -- in.doc outdir/ --parts
//! ```
//!
//! `--parts` writes every part of the package as a file under `outdir/` (what the parity facet
//! `parts` digests), to diff against the parts of the TS converter.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

#[cfg(not(feature = "doc"))]
fn main() {
    eprintln!("build with --features doc");
}

#[cfg(feature = "doc")]
fn main() {
    use std::path::Path;

    use uo_docx::doc::{DocLimits, convert_doc_to_docx};
    use uo_opc::Package;
    use uo_opc::package::OpenOptions;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output, rest @ ..] = args.as_slice() else {
        eprintln!("usage: doc_to_docx in.doc out.docx | in.doc outdir/ --parts");
        std::process::exit(2);
    };
    let bytes = std::fs::read(input).unwrap();
    let docx = match convert_doc_to_docx(&bytes, &DocLimits::DEFAULT) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{input}: {} ({e})", e.class());
            std::process::exit(1);
        }
    };
    if rest.iter().any(|a| a == "--parts") {
        let pkg = Package::open(&docx, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
        let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
        for n in &names {
            let path = Path::new(output).join(n);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, pkg.part(n).unwrap().unwrap_or(&[])).unwrap();
        }
        println!("{}", names.join("\n"));
    } else {
        std::fs::write(output, docx).unwrap();
    }
}
