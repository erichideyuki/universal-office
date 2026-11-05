//! M3 of the model size (record `uo-docx.md`, "R5 follow-ups" → "Model size — analysis" and "Model
//! size — P1 done"): the bytes of a read model per document class, with the props `Arc`s interned
//! by the reader (`read::intern`) and each paragraph's `Vec<Run>` at its exact capacity.
//!
//! - **Budgets** (asserted): the model of each class within the bytes of the record, below the TS
//!   heap of R5's M3 (`heapUsed` of `readFullDocument`) on the synthetic classes.
//! - **The interning is exact and complete** (asserted): an independent equality — the `Debug` text,
//!   which shows every field, the cleared mask and the key order — counts the distinct props values
//!   per document; the reader allocates exactly one `Arc` per distinct value (fewer would mean two
//!   values that differ were merged; more, a value left out). Interning the read model again, as the
//!   P0 prototype did after the read, saves nothing.
//!
//! The numbers are printed with `--nocapture`:
//!
//!   cargo test --release -p uo-docx --all-features --test memory_model_size -- --nocapture
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::fmt::Write as _;
use std::mem::size_of;
use std::sync::Arc;

use uo_alloc::measure;
use uo_docx::model::props::{PPr, RPr, TblPr, TcPr, TrPr};
use uo_docx::model::run::{RunExt, ThemeColorAttrs};
use uo_docx::model::table::{Cell, Row, Table};
use uo_docx::model::{Document, ParaProps, Paragraph, Run, RunProps, Story};
use uo_docx::read::{ReadOptions, read_document};
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_testkit::repo_root;

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";

fn pkg_of(body: &str) -> Package {
    Package::from_parts(
        [
            ("word/document.xml".to_owned(), format!("<w:document {NS}><w:body>{body}</w:body></w:document>").into_bytes()),
            ("word/_rels/document.xml.rels".to_owned(), b"<Relationships/>".to_vec()),
        ],
        &uo_opc::Limits::DEFAULT,
        &OpenOptions::default(),
    )
}

/// The M3 classes of R5 (`memory_r5.rs`): paragraphs with a bold run and a plain run, a table.
fn paragraphs(n: usize) -> Package {
    let mut body = String::new();
    for i in 0..n {
        let _ = write!(
            body,
            "<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>paragraph {i} with some text </w:t></w:r><w:r><w:t>and more</w:t></w:r></w:p>"
        );
    }
    pkg_of(&body)
}

fn tables(rows: usize) -> Package {
    let mut body =
        String::from("<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w=\"1000\"/><w:gridCol w:w=\"1000\"/><w:gridCol w:w=\"1000\"/></w:tblGrid>");
    for i in 0..rows {
        body.push_str("<w:tr>");
        for j in 0..3 {
            let _ = write!(
                body,
                "<w:tc><w:tcPr><w:tcW w:w=\"1000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>cell {i}.{j}</w:t></w:r></w:p></w:tc>"
            );
        }
        body.push_str("</w:tr>");
    }
    body.push_str("</w:tbl>");
    pkg_of(&body)
}

/// Pointer and value counts of one kind of `Arc` in the model (summed over documents: an
/// interning table is per read).
#[derive(Default)]
struct Kind {
    /// `Arc`s held (one per paragraph or run that has one)
    held: u64,
    /// distinct allocations, summed per document
    allocations: u64,
    /// distinct values, summed per document (the `Debug` text, which includes the cleared mask and
    /// the key order: the equality an interning table must use)
    distinct: u64,
    /// documents where the allocations and the distinct values differ
    unequal: Vec<String>,
    ptrs: HashSet<usize>,
    values: HashSet<String>,
}

impl Kind {
    fn add<T: Debug>(&mut self, a: &Arc<T>) {
        self.held += 1;
        self.ptrs.insert(Arc::as_ptr(a).addr());
        self.values.insert(format!("{:?}", **a));
    }

    fn end_document(&mut self, doc: usize) {
        if self.ptrs.len() != self.values.len() {
            self.unequal.push(format!("document {doc}: {} allocations, {} values", self.ptrs.len(), self.values.len()));
        }
        self.allocations += self.ptrs.len() as u64;
        self.distinct += self.values.len() as u64;
        self.ptrs.clear();
        self.values.clear();
    }
}

#[derive(Default)]
struct Census {
    documents: usize,
    paragraphs: u64,
    runs: u64,
    run_slots: u64,
    text_units: u64,
    exts: u64,
    para_props: Kind,
    ppr: Kind,
    run_props: Kind,
    rpr: Kind,
    tc_pr: Kind,
    tr_pr: Kind,
    tbl_pr: Kind,
}

impl Census {
    fn kinds(&self) -> [(&'static str, &Kind, usize); 7] {
        [
            ("ParaProps", &self.para_props, size_of::<ParaProps>()),
            ("PPr", &self.ppr, size_of::<PPr>()),
            ("RunProps", &self.run_props, size_of::<RunProps>()),
            ("RPr", &self.rpr, size_of::<RPr>()),
            ("TcPr", &self.tc_pr, size_of::<TcPr>()),
            ("TrPr", &self.tr_pr, size_of::<TrPr>()),
            ("TblPr", &self.tbl_pr, size_of::<TblPr>()),
        ]
    }

    fn end_document(&mut self) {
        let doc = self.documents;
        self.documents += 1;
        for k in
            [&mut self.para_props, &mut self.ppr, &mut self.run_props, &mut self.rpr, &mut self.tc_pr, &mut self.tr_pr, &mut self.tbl_pr]
        {
            k.end_document(doc);
        }
    }

    fn story(&mut self, s: &Story) {
        for p in s.paras.iter() {
            self.paragraph(p);
        }
        for a in s.tables.iter() {
            if let Some(x) = &a.table.tbl_pr {
                self.tbl_pr.add(x);
            }
            for row in &a.table.rows {
                if let Some(x) = &row.tr_pr {
                    self.tr_pr.add(x);
                }
                if let Some(x) = &row.tbl_pr_ex {
                    self.tbl_pr.add(x);
                }
                for c in &row.cells {
                    if let Some(x) = &c.tc_pr {
                        self.tc_pr.add(x);
                    }
                    self.story(&c.content);
                }
            }
        }
    }

    fn paragraph(&mut self, p: &Paragraph) {
        self.paragraphs += 1;
        self.run_slots += p.runs.capacity() as u64;
        self.para_props.add(&p.props);
        if let Some(x) = &p.ppr {
            self.ppr.add(x);
        }
        for r in &p.runs {
            self.run(r);
        }
    }

    fn run(&mut self, r: &Run) {
        self.runs += 1;
        self.text_units += r.text.len() as u64;
        self.run_props.add(&r.props);
        if let Some(x) = &r.rpr {
            self.rpr.add(x);
        }
        if let Some(ext) = r.ext() {
            self.exts += 1;
            if let Some(tb) = ext.textbox.as_deref() {
                self.story(&tb.content);
            }
            for f in ext.field_result.iter().flatten() {
                self.run_props.add(&f.props);
                if let Some(x) = &f.rpr {
                    self.rpr.add(x);
                }
            }
            if let Some(g) = ext.group.as_deref() {
                for it in &g.items {
                    self.run_props.add(&it.run.props);
                }
            }
        }
    }
}

/// The P0 prototype: interns equal `Arc`s of a read model after the read (by the `Debug` text, see
/// [`Kind::values`]).
struct Interner<T> {
    table: HashMap<String, Arc<T>>,
}

impl<T: Debug> Interner<T> {
    fn new() -> Self {
        Interner { table: HashMap::new() }
    }

    fn intern(&mut self, a: &mut Arc<T>) {
        let key = format!("{:?}", **a);
        match self.table.get(&key) {
            Some(shared) => *a = Arc::clone(shared),
            None => {
                self.table.insert(key, Arc::clone(a));
            }
        }
    }
}

struct Interners {
    para_props: Interner<ParaProps>,
    ppr: Interner<PPr>,
    run_props: Interner<RunProps>,
    rpr: Interner<RPr>,
    tc_pr: Interner<TcPr>,
}

impl Interners {
    fn story(&mut self, s: &mut Story) {
        for i in 0..s.paras.len() {
            let p = s.paras.make_mut(i).unwrap();
            self.para_props.intern(&mut p.props);
            if let Some(x) = p.ppr.as_mut() {
                self.ppr.intern(x);
            }
            p.runs.shrink_to_fit();
            for r in &mut p.runs {
                self.run_props.intern(&mut r.props);
                if let Some(x) = r.rpr.as_mut() {
                    self.rpr.intern(x);
                }
                if let Some(tb) = r.ext.as_mut().and_then(|e| e.textbox.as_mut()) {
                    self.story(&mut tb.content);
                }
            }
        }
        for i in 0..s.tables.len() {
            let a = s.tables.make_mut(i).unwrap();
            for row in &mut Arc::make_mut(&mut a.table).rows {
                for c in &mut row.cells {
                    if let Some(x) = c.tc_pr.as_mut() {
                        self.tc_pr.intern(x);
                    }
                    self.story(&mut c.content);
                }
            }
        }
    }
}

fn read(pkg: &Package) -> Document {
    read_document(pkg, &ReadOptions::default()).unwrap()
}

/// The read model interned again after the read (the P0 prototype).
fn interned_again(pkg: &Package) -> Document {
    let mut doc = read(pkg);
    let mut i = Interners {
        para_props: Interner::new(),
        ppr: Interner::new(),
        run_props: Interner::new(),
        rpr: Interner::new(),
        tc_pr: Interner::new(),
    };
    i.story(&mut doc.body);
    doc
}

/// What one class measured.
struct Measured {
    model: i64,
    again: i64,
    census: Census,
}

fn measure_class(pkgs: &[Package]) -> Measured {
    let (mut model, mut again) = (0i64, 0i64);
    let mut census = Census::default();
    for pkg in pkgs {
        drop(read(pkg));
        let (doc, u) = measure(|| read(pkg));
        model += u.live_delta;
        census.story(&doc.body);
        census.end_document();
        drop(doc);
        let (doc, u) = measure(|| interned_again(pkg));
        again += u.live_delta;
        drop(doc);
    }
    Measured { model, again, census }
}

fn report(name: &str, pkgs: &[Package], m: &Measured) -> String {
    let c = &m.census;
    let mut s = String::new();
    let _ = writeln!(s, "## {name} ({} documents)", pkgs.len());
    let _ = writeln!(s, "model {} B; interned again after the read {} B", m.model, m.again);
    let _ = writeln!(
        s,
        "paragraphs {}, runs {} (Vec<Run> slots {}), text {} B, runs with ext {}",
        c.paragraphs,
        c.runs,
        c.run_slots,
        c.text_units * 2,
        c.exts
    );
    for (kind, k, size) in c.kinds() {
        let _ = writeln!(s, "{kind}: held {}, allocations {}, distinct values {} ({size} B each)", k.held, k.allocations, k.distinct);
    }
    s
}

/// The checks every class passes: one allocation per distinct value, nothing left to intern, every
/// `Vec<Run>` at its exact capacity.
fn check(name: &str, m: &Measured) {
    for (kind, k, _) in m.census.kinds() {
        assert!(k.unequal.is_empty(), "{name}, {kind}: {:?}", &k.unequal[..k.unequal.len().min(5)]);
    }
    assert_eq!(m.again, m.model, "{name}: interning the read model again changed its size");
    assert_eq!(m.census.run_slots, m.census.runs, "{name}: Vec<Run> capacity");
}

fn oracle(step: usize) -> Vec<Package> {
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("write/packages/docx-layout/test/oracle");
    let mut all: Vec<std::path::PathBuf> = Vec::new();
    for d in std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()) {
        for f in std::fs::read_dir(&d).unwrap().filter_map(Result::ok).map(|e| e.path()) {
            if f.extension().is_some_and(|e| e == "docx") {
                all.push(f);
            }
        }
    }
    all.sort();
    all.into_iter()
        .step_by(step)
        .filter_map(|p| Package::open(&std::fs::read(p).ok()?, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).ok())
        .collect()
}

/// R5's M3 TS heap of the class (`js/measure-memory-full.mjs`, Node 24.15 x64, `heapUsed` after a
/// full GC with `readFullDocument` alive, median of 5).
const TS_HEAP_2000_PARAGRAPHS: i64 = 2_571_864;
const TS_HEAP_3000_ROWS: i64 = 9_429_568;

#[test]
fn m3_model_size_with_the_props_interned() {
    println!(
        "sizes: Paragraph {} Run {} RunExt {} ParaProps {} RunProps {} PPr {} RPr {} ThemeColorAttrs {} Cell {} Row {} Table {} Story {}",
        size_of::<Paragraph>(),
        size_of::<Run>(),
        size_of::<RunExt>(),
        size_of::<ParaProps>(),
        size_of::<RunProps>(),
        size_of::<PPr>(),
        size_of::<RPr>(),
        size_of::<ThemeColorAttrs>(),
        size_of::<Cell>(),
        size_of::<Row>(),
        size_of::<Table>(),
        size_of::<Story>()
    );
    // the budgets of the record (native x86-64; before P1 4,412,404, 17,566,308 and 44,948,929 B,
    // after 636,149, 4,732,528 and 23,083,495 B)
    let classes: [(&str, Vec<Package>, i64); 3] = [
        ("text, 2,000 paragraphs", vec![paragraphs(2_000)], 700_000),
        ("tables, 3,000 rows x 3", vec![tables(3_000)], 5_200_000),
        ("Word oracle, one in four", oracle(4), 25_000_000),
    ];
    let mut models = Vec::new();
    for (name, pkgs, budget) in &classes {
        let m = measure_class(pkgs);
        print!("{}", report(name, pkgs, &m));
        check(name, &m);
        assert!(m.model <= *budget, "{name}: model {} B, budget {budget} B", m.model);
        models.push(m.model);
    }
    // below the TS heap (0.25x and 0.50x measured)
    assert!(models[0] * 3 < TS_HEAP_2000_PARAGRAPHS, "2,000 paragraphs: {} B, the TS heap {TS_HEAP_2000_PARAGRAPHS} B", models[0]);
    assert!(models[1] * 3 / 2 < TS_HEAP_3000_ROWS, "3,000 rows: {} B, the TS heap {TS_HEAP_3000_ROWS} B", models[1]);
}
