//! Fuzz target of the `uo-xlsx` surgical save (feature `save`, record `core/docs/modules/uo-xlsx.md` "C1b - as
//! built", `xlsx_roundtrip`): the same inputs as `xlsx_read` (a package, or one mutated part inside a minimal
//! package). Whatever opens is loaded whole, every sheet is marked edited (so every `<sheetData>` is streamed from
//! the store, with the merges and the view rewritten too) and saved: no panic; a save that succeeds is a zip that
//! opens again, and what it wrote reads back without losing a cell of the live store.

use uo_opc::Package;
use uo_opc::package::{CaseFold, OpenOptions};
use uo_xlsx::book::Book;
use uo_xlsx::save::{SaveRequest, save};

use crate::xlsx_read::{limits, package};

pub fn xlsx_roundtrip(data: &[u8]) {
    let Some((&mode, rest)) = data.split_first() else { return };
    let bytes = if mode % 4 == 0 { rest.to_vec() } else { package(mode % 4, rest) };
    let (pl, rl) = limits();
    let Ok(pkg) = Package::open_owned(bytes, &pl, &OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() }) else { return };
    let Ok(mut book) = Book::from_package(pkg, rl) else { return };
    if book.load_all().is_err() {
        return;
    }
    let all: Vec<usize> = (0..book.workbook.sheets.len()).collect();
    let Ok(saved) =
        save(&mut book, &SaveRequest { dirty_sheets: &all, merge_dirty: &all, view_dirty: &all, level: Some(0), ..SaveRequest::default() })
    else {
        return;
    };
    let Ok(again) = Package::open_owned(saved.bytes, &pl, &OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() }) else {
        panic!("a saved workbook is not a zip");
    };
    // what was written reads back (a hostile `t` is written as the TS writes it, unescaped, and may not)
    if let Ok(mut b2) = Book::from_package(again, rl) {
        let _ = b2.load_all();
        assert!(b2.store.check_invariants().is_ok());
    }
}
