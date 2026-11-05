//! Frontier F6 (tables): the M1 memory gate (no leak per operation: `layout_table`, every row
//! split, `split_content` with nested rows, `at_page_top`, `merge_text_first`, `drop_moved`), the
//! M3 numbers, and the no-panic loops over hostile tables (huge and negative spans, NaN/±∞/zero
//! widths, deep nesting, many rows, broken merges). The bit-exact parity is `tests/f6_parity.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use uo_alloc::assert_no_leak;
use uo_docx::DocView;
use uo_docx::ids::{ParaId, TableId};
use uo_docx::model::story::TableAnchor;
use uo_docx::model::{Document, Run};
use uo_docx_layout::context::DocContext;
use uo_docx_layout::engine::{Cx, PassInput};
use uo_docx_layout::error::{LResult, LayoutError, LimitKind};
use uo_docx_layout::fonts::{FaceResolverCache, FaceTable, FontAnswer, FontProvider};
use uo_docx_layout::hyphenate::HyphenSessions;
use uo_docx_layout::items::{ItemSide, PassLine};
use uo_docx_layout::keys::{LParaId, ObjId};
use uo_docx_layout::ldoc::LDoc;
use uo_docx_layout::model::{
    Block, BorderSpec, Cell, CellKey, JsNumber, Paragraph, Row, ShadingSpec, Story, Table, TblBorders, TblPr, TcPr, TrHeight, TrPr,
    WidthSpec,
};
use uo_docx_layout::options::{Budget, LayoutLimits, LayoutOptions};
use uo_docx_layout::output::{LineRef, LineStory, RefPath, TabAlign, TabStop};
use uo_docx_layout::para::{Ch, Glyph, GlyphFlags, GlyphKind, LineRule, ParaEnv, PreparedPara, RunStyle, StyleIx};
use uo_docx_layout::table::{
    CellContent, CellEnv, CellLayouter, NestLine, NestRow, ParaSplitInfo, TableLayout, at_page_top, drop_moved, layout_table,
    merge_text_first, split_content,
};
use uo_docx_layout::word;
use uo_render::types::Item;
use uo_render::{FaceId, OptF64, Resources};

// ── a world: a document, the per-call state and a synthetic engine ───────────────────────────

struct NoFonts;

impl FontProvider for NoFonts {
    fn face(&self, _family: &str, _bold: bool, _italic: bool) -> Option<Arc<FontAnswer>> {
        None
    }
}

/// The engine stand-in: a cell's paragraphs become lines of 240 twips (one per 120 twips of text
/// per line width), each with a background rect; a nested table is laid out through
/// `layout_table` and placed as the engine does (rows shifted, `nest` rows with identities).
struct Synth {
    ctx: DocContext,
    cells: u64,
}

impl Synth {
    fn new(o: &LayoutOptions) -> Synth {
        Synth {
            ctx: DocContext::new(word::StyleResolver::new(), word::numbering_state(&o.lang, &o.limits), word::revision_display(None)),
            cells: 0,
        }
    }
}

fn sub_ref(at: &LineRef, k: u32) -> LineRef {
    let mut p = at.path.0.to_vec();
    p.push(k);
    LineRef { story: at.story, path: RefPath(p.into()) }
}

fn line(at: LineRef, y: f64, h: f64, w: f64) -> PassLine {
    PassLine {
        at,
        start: 0,
        end: 1,
        x: 0.0,
        y,
        height: h,
        baseline: y + h * 0.8,
        width: w,
        chars: None,
        hyph: None,
        rev: None,
        frame: None,
        pfmt: None,
        meta: None,
        obj: None,
    }
}

impl CellLayouter for Synth {
    fn ctx(&mut self) -> &mut DocContext {
        &mut self.ctx
    }

    fn layout_cell(
        &mut self,
        cx: &mut Cx<'_, '_>,
        cell: &Cell,
        _key: CellKey,
        x: f64,
        width: f64,
        _env: &CellEnv,
        at: &LineRef,
    ) -> LResult<CellContent> {
        self.cells += 1;
        let fill = cx.res.symbols.intern_str("cccccc").unwrap();
        let mut c = CellContent::default();
        let mut paras = IndexMap::new();
        let mut nest = Vec::new();
        let mut y = 0.0;
        let mut next = 1u32;
        for (bi, b) in cell.content.blocks().enumerate() {
            let r = sub_ref(at, u32::try_from(bi).unwrap());
            match b {
                Block::Paragraph(p) => {
                    let n = p.text_len() as f64;
                    let per = if width > 0.0 { width } else { 1.0 };
                    let lines = uo_jsnum::math::min(uo_jsnum::math::max(uo_jsnum::math::ceil(n * 120.0 / per), 1.0), 200.0);
                    let k = lines as u32;
                    for i in 0..k {
                        c.items.list.push_plain(Item::Rect { x, y, w: width, h: 240.0, fill, alpha: OptF64::NONE });
                        c.items.side.push(ItemSide::default());
                        c.lines.push(line(r.clone(), y, 240.0, uo_jsnum::math::min(width, n * 120.0)));
                        let _ = i;
                        y += 240.0;
                    }
                    paras.insert(r, ParaSplitInfo { lines, widow: true, after: 120.0, band: OptF64::NONE });
                    y += 120.0;
                }
                Block::Table(t) => {
                    let tl = layout_table(cx, self, t, x, width, &r)?;
                    for row in tl.rows {
                        let mut ids = Vec::new();
                        for src in [&row.items, &row.border_items] {
                            for i in 0..src.len() {
                                c.items.push_shifted(src, i, y, 0.0).unwrap();
                                c.items.side.last_mut().unwrap().obj = Some(ObjId(next));
                                ids.push(ObjId(next));
                                next += 1;
                            }
                        }
                        let mut nl = Vec::new();
                        for l in &row.lines {
                            let mut s = l.shifted(y);
                            s.obj = Some(ObjId(next));
                            nl.push(NestLine { obj: ObjId(next), at: s.at.clone() });
                            next += 1;
                            c.lines.push(s);
                        }
                        let h = row.height;
                        nest.push(NestRow { top: y, row: Box::new(row), items: ids, lines: nl });
                        y += h;
                    }
                }
            }
        }
        c.height = y;
        if !paras.is_empty() {
            c.paras = Some(Arc::new(paras));
        }
        if !nest.is_empty() {
            c.nest = Some(nest);
        }
        Ok(c)
    }

    fn prepare(&mut self, _cx: &mut Cx<'_, '_>, para: &Paragraph, _env: &ParaEnv) -> LResult<PreparedPara> {
        let face = FaceId::from_index(0).unwrap();
        let mut glyphs = Vec::new();
        for r in &para.runs {
            for &u in r.text.units() {
                let kind = match u {
                    0x20 => GlyphKind::Space,
                    0x09 => GlyphKind::Tab,
                    0x0b => GlyphKind::Lb,
                    _ => GlyphKind::Char,
                };
                let mut g = Glyph::new(kind, Ch::default(), u32::from(u), face, StyleIx(0));
                g.adv = 110.0;
                if u == 0x2d {
                    g.flags = GlyphFlags(GlyphFlags::BREAK_AFTER);
                }
                glyphs.push(g);
            }
        }
        Ok(PreparedPara {
            para: LParaId(0),
            ppr: Arc::default(),
            style_id: None,
            styles: vec![RunStyle { size_pt: 11.0, ..RunStyle::default() }],
            mark_style: StyleIx(0),
            mark_face: face,
            glyphs,
            marker: None,
            ind_left: 0.0,
            ind_right: 0.0,
            first_line: 0.0,
            jc: "left".into(),
            tabs: vec![TabStop { val: TabAlign::Left, pos: 1440.0, leader: None }],
            space_before: 0.0,
            space_after: 0.0,
            before_auto: false,
            after_auto: false,
            line_rule: LineRule::Auto,
            line: 0.0,
            contextual: false,
            keep_next: false,
            keep_lines: false,
            widow_control: true,
            page_break_before: false,
            empty: false,
            rev_bars: None,
            math: Vec::new(),
        })
    }

    fn visibility(&mut self, _cx: &mut Cx<'_, '_>, para: &Paragraph) -> LResult<(bool, bool)> {
        let hidden = para.runs.first().is_some_and(|r| r.text.units().first() == Some(&u16::from(b'~')));
        Ok((hidden, !hidden))
    }
}

/// Runs `f` with a `Cx` over `doc` (fresh per-call state, dropped at the end).
fn with_cx<T>(doc: &Document, o: &LayoutOptions, f: impl FnOnce(&mut Cx<'_, '_>) -> T) -> T {
    let ldoc = LDoc::empty(DocView::new(doc));
    let input = PassInput::default();
    let mut fonts = FaceTable::new();
    let mut resolver = FaceResolverCache::new();
    let mut res = Resources::new();
    let mut hyph = HyphenSessions::new(Arc::new(uo_fonts::hyphen::HyphenationRegistry::new()));
    let mut budget = Budget::unlimited();
    let mut seq = 0u32;
    let mut cx = Cx {
        ldoc: &ldoc,
        input: &input,
        opts: o,
        fonts: &mut fonts,
        provider: &NoFonts,
        resolver: &mut resolver,
        res: &mut res,
        hyph: &mut hyph,
        budget: &mut budget,
        depth: 0,
        line_seq: &mut seq,
    };
    f(&mut cx)
}

// ── table builders ───────────────────────────────────────────────────────────────────────────

fn n(v: f64) -> JsNumber {
    JsNumber::Num(v)
}

fn dxa(w: f64) -> WidthSpec {
    WidthSpec { w: n(w), r#type: "dxa".into(), ..WidthSpec::default() }
}

fn border(val: &str, sz: f64) -> Box<BorderSpec> {
    Box::new(BorderSpec { val: val.into(), sz: Some(n(sz)), color: Some("FF0000".into()), ..BorderSpec::default() })
}

fn para(id: u32, text: &str) -> Paragraph {
    Paragraph { id: ParaId(id), runs: vec![Run { text: text.into(), ..Run::default() }], ..Paragraph::default() }
}

fn cell(text: &str, tc: TcPr) -> Cell {
    let mut content = Story::default();
    content.paras.push(para(1, text));
    Cell { grid_span: n(1.0), tc_pr: Some(Arc::new(tc)), content, ..Cell::default() }
}

fn row(cells: Vec<Cell>, tr: Option<TrPr>) -> Row {
    Row { cells, tr_pr: tr.map(Arc::new), ..Row::default() }
}

fn table(id: u32, rows: Vec<Row>, grid: &[f64], tbl: TblPr) -> Table {
    Table { rows, grid_cols_twips: grid.iter().map(|g| n(*g)).collect(), tbl_pr: Some(Arc::new(tbl)), id: TableId(id), ..Table::default() }
}

fn all_borders() -> TblPr {
    TblPr {
        tbl_borders: Some(Box::new(TblBorders {
            top: Some(border("single", 8.0)),
            left: Some(border("single", 8.0)),
            bottom: Some(border("double", 4.0)),
            right: Some(border("single", 12.0)),
            inside_h: Some(border("single", 4.0)),
            inside_v: Some(border("dotted", 4.0)),
            ..TblBorders::default()
        })),
        ..TblPr::default()
    }
}

/// The tables of the M1 scenario: every path of `layoutTable` once.
fn tables() -> Vec<Table> {
    let long = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore";
    let shade = |hex: &str| Some(Box::new(ShadingSpec { val: Some("clear".into()), fill: Some(hex.into()), ..ShadingSpec::default() }));
    let mut out = Vec::new();
    // 1. plain 3×3 with borders, autofit, fills, a dark cell
    let rows = (0..3)
        .map(|r| {
            row(
                (0..3)
                    .map(|c| {
                        cell(
                            if (r + c) % 2 == 0 { long } else { "x" },
                            TcPr { shd: if c == 1 { shade("1F1F1F") } else { None }, ..TcPr::default() },
                        )
                    })
                    .collect(),
                None,
            )
        })
        .collect();
    out.push(table(1, rows, &[2000.0, 3000.0, 2000.0], all_borders()));
    // 2. vertical merges with a fill, exact and atLeast heights, vertical text
    let vm = |v: &str| TcPr { v_merge: Some(v.into()), shd: shade("DDEEFF"), ..TcPr::default() };
    let rows = vec![
        row(
            vec![
                cell(long, vm("restart")),
                cell("a", TcPr::default()),
                cell("v", TcPr { text_direction: Some("btLr".into()), ..TcPr::default() }),
            ],
            None,
        ),
        row(
            vec![cell("", vm("continue")), cell(long, TcPr::default()), cell("w", TcPr::default())],
            Some(TrPr { tr_height: Some(TrHeight { val: n(600.0), h_rule: "atLeast".into(), ..TrHeight::default() }), ..TrPr::default() }),
        ),
        row(
            vec![cell("", vm("continue")), cell("b", TcPr::default()), cell("y", TcPr::default())],
            Some(TrPr { tr_height: Some(TrHeight { val: n(400.0), h_rule: "exact".into(), ..TrHeight::default() }), ..TrPr::default() }),
        ),
        row(
            vec![
                cell(long, TcPr::default()),
                cell(long, vm("restart")),
                cell("t", TcPr { text_direction: Some("tbRl".into()), v_merge: Some("restart".into()), ..TcPr::default() }),
            ],
            None,
        ),
        row(
            vec![
                cell("z", TcPr::default()),
                cell("", vm("continue")),
                cell("", TcPr { v_merge: Some("continue".into()), ..TcPr::default() }),
            ],
            None,
        ),
    ];
    out.push(table(2, rows, &[2500.0, 2500.0, 2500.0], all_borders()));
    // 3. right to left, separated borders, fixed layout with tcW, gridBefore/After
    let tc = |w: f64| TcPr {
        tc_w: Some(dxa(w)),
        tc_borders: Some(Box::new(uo_docx_layout::model::TcBorders { left: Some(border("single", 24.0)), ..Default::default() })),
        ..TcPr::default()
    };
    let gb = TrPr {
        grid_before: Some(n(1.0)),
        w_before: Some(dxa(500.0)),
        grid_after: Some(n(1.0)),
        w_after: Some(dxa(300.0)),
        ..TrPr::default()
    };
    let rows = vec![
        row(vec![cell(long, tc(2000.0)), cell("b", tc(1500.0))], Some(gb.clone())),
        row(vec![cell("c", tc(2000.0)), cell(long, tc(1500.0))], Some(gb)),
    ];
    let tp = TblPr {
        bidi_visual: Some(true),
        tbl_cell_spacing: Some(dxa(40.0)),
        tbl_layout: Some("fixed".into()),
        shd: shade("EEEEEE"),
        jc: Some("center".into()),
        ..all_borders()
    };
    out.push(table(3, rows, &[500.0, 2000.0, 1500.0, 300.0], tp));
    // 4. a nested table in a cell (split as a table row), a hidden row, noWrap
    let inner_rows = (0..4).map(|_| row(vec![cell(long, TcPr::default()), cell(long, TcPr::default())], None)).collect();
    let inner = table(5, inner_rows, &[1500.0, 1500.0], all_borders());
    let mut nested = cell("before", TcPr::default());
    nested.content.tables.push(TableAnchor { before_para: 1, table: Arc::new(inner) });
    nested.content.paras.push(para(2, "after"));
    let hidden = TrPr { hidden: Some(true), ..TrPr::default() };
    let rows = vec![
        row(vec![nested, cell("n", TcPr { no_wrap: Some(true), ..TcPr::default() })], None),
        row(vec![cell("~gone", TcPr::default()), cell("~gone", TcPr::default())], Some(hidden)),
        row(vec![cell(long, TcPr::default()), cell(long, TcPr::default())], None),
    ];
    out.push(table(4, rows, &[3500.0, 3500.0], all_borders()));
    out
}

/// Every operation of the module over the scenario tables (results dropped).
fn scenario(doc: &Document, ts: &[Table], o: &LayoutOptions) -> (usize, usize) {
    with_cx(doc, o, |cx| {
        let mut host = Synth::new(o);
        let mut splits = 0usize;
        let mut rows = 0usize;
        for t in ts {
            let at = LineRef { story: LineStory::Body, path: RefPath(vec![t.id.0].into()) };
            let tl: TableLayout = layout_table(cx, &mut host, t, 1440.0, 9000.0, &at).unwrap();
            rows += tl.rows.len();
            let mut moved = IndexSet::new();
            for (ri, r) in tl.rows.iter().enumerate() {
                let r2 = merge_text_first(&tl.rows, ri, r, &mut moved, &mut |_| true).unwrap();
                let r3 = drop_moved(r2.as_ref().unwrap_or(r), &moved);
                let _ = at_page_top(r3.as_ref().or(r2.as_ref()).unwrap_or(r)).unwrap();
                if let Some(sp) = &r.split {
                    for avail in [100.0, 400.0, 700.0, 1500.0] {
                        if let Some((a, b)) = sp.split(cx, avail).unwrap() {
                            splits += 1;
                            let _ = at_page_top(&a).unwrap();
                            if let Some(sp2) = &b.split {
                                let _ = sp2.split(cx, avail).unwrap();
                            }
                        }
                    }
                }
            }
            // a cell content with a nested row cut through (splitAtNest)
            let cellc = &t.rows[0].cells[0];
            let key = CellKey { table: t.id, row: 0, cell: 0 };
            let c = host.layout_cell(cx, cellc, key, 0.0, 3000.0, &CellEnv::default(), &at).unwrap();
            for avail in [200.0, 500.0, 900.0, 2000.0] {
                if let Some((a, b)) = split_content(cx, &c, avail).unwrap() {
                    let _ = split_content(cx, &b, avail).unwrap();
                    drop(a);
                }
            }
        }
        (rows, splits)
    })
}

#[test]
fn m1_every_operation_leaves_nothing() {
    let doc = Document::default();
    let ts = tables();
    let o = LayoutOptions::default();
    let (rows, splits) = scenario(&doc, &ts, &o);
    assert!(rows >= 12 && splits >= 4, "{rows} rows, {splits} splits");
    let u = assert_no_leak(|| scenario(&doc, &ts, &o));
    eprintln!("M1 tables ({} tables, {rows} rows, {splits} splits; layout + splits + row ops + splitContent): {u:?}", ts.len());
    // one big table alone (M3: per-row cost)
    let long = "lorem ipsum dolor sit amet consectetur adipiscing elit";
    let big = table(
        9,
        (0..500).map(|_| row(vec![cell(long, TcPr::default()), cell("x", TcPr::default()), cell(long, TcPr::default())], None)).collect(),
        &[3000.0, 1000.0, 3000.0],
        all_borders(),
    );
    let run = || {
        with_cx(&doc, &o, |cx| {
            let mut host = Synth::new(&o);
            let at = LineRef { story: LineStory::Body, path: RefPath(vec![0].into()) };
            layout_table(cx, &mut host, &big, 0.0, 9000.0, &at).unwrap().rows.len()
        })
    };
    assert_eq!(run(), 500);
    let u = assert_no_leak(run);
    eprintln!("M1/M3 500-row table: {u:?}");
}

// ── hostile tables ──────────────────────────────────────────────────────────────────────────

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn one_in(&mut self, k: u64) -> bool {
        self.next().is_multiple_of(k)
    }
    fn pick<T: Clone>(&mut self, v: &[T]) -> T {
        v[(self.next() % v.len() as u64) as usize].clone()
    }
    fn num(&mut self) -> f64 {
        self.pick(&[0.0, -0.0, 1.0, -1.0, 3.0, 100.0, 2000.0, -5000.0, 1e9, 1e300, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.5])
    }
}

fn hostile(rng: &mut Rng, id: u32, depth: u32, wild: bool) -> Table {
    let nrows = rng.pick(&[0usize, 1, 2, 3, 7]);
    let mut rows = Vec::new();
    for _ in 0..nrows {
        let ncells = rng.pick(&[0usize, 1, 2, 4]);
        let mut cells = Vec::new();
        for _ in 0..ncells {
            let tc = TcPr {
                grid_span: if rng.one_in(4) {
                    Some(n(if wild { rng.pick(&[0.0, -3.0, 2.0, 5.0, f64::NAN, 1e9]) } else { rng.pick(&[0.0, 1.0, 2.0, 3.0]) }))
                } else {
                    None
                },
                v_merge: if rng.one_in(3) { Some(rng.pick(&["restart", "continue", "x"]).into()) } else { None },
                h_merge: if rng.one_in(6) { Some("continue".into()) } else { None },
                tc_w: if rng.one_in(2) {
                    Some(WidthSpec { w: n(rng.num()), r#type: rng.pick(&["dxa", "pct", "auto", "nil"]).into(), ..WidthSpec::default() })
                } else {
                    None
                },
                text_direction: if rng.one_in(5) { Some(rng.pick(&["btLr", "tbRl", "lrTb"]).into()) } else { None },
                no_wrap: Some(rng.one_in(4)),
                tc_borders: if rng.one_in(3) {
                    Some(Box::new(uo_docx_layout::model::TcBorders {
                        top: Some(border(rng.pick(&["single", "double", "triple", "nil"]), rng.num())),
                        bottom: Some(border("thinThickSmallGap", rng.num())),
                        tl2br: Some(border("single", 4.0)),
                        ..Default::default()
                    }))
                } else {
                    None
                },
                ..TcPr::default()
            };
            let mut c = cell(rng.pick(&["", "a", "lorem ipsum dolor sit amet", "~h"]), tc);
            if depth > 0 && rng.one_in(3) {
                let inner = hostile(rng, id * 10 + 1, depth - 1, wild);
                c.content.tables.push(TableAnchor { before_para: 0, table: Arc::new(inner) });
            }
            cells.push(c);
        }
        let tr = TrPr {
            grid_before: if rng.one_in(4) { Some(n(if wild { rng.pick(&[1.0, -2.0, 1e9, f64::NAN]) } else { 1.0 })) } else { None },
            w_before: Some(dxa(rng.num())),
            grid_after: if rng.one_in(5) { Some(n(rng.pick(&[1.0, 3.0]))) } else { None },
            tr_height: if rng.one_in(3) {
                Some(TrHeight { val: n(rng.num()), h_rule: rng.pick(&["exact", "atLeast", "auto"]).into(), ..TrHeight::default() })
            } else {
                None
            },
            cant_split: Some(rng.one_in(5)),
            hidden: Some(rng.one_in(7)),
            tbl_cell_spacing: if rng.one_in(6) { Some(dxa(rng.num())) } else { None },
            jc: if rng.one_in(4) { Some(rng.pick(&["center", "right", "end"]).into()) } else { None },
            ..TrPr::default()
        };
        rows.push(row(cells, Some(tr)));
    }
    let grid: Vec<f64> = (0..rng.pick(&[0usize, 1, 3])).map(|_| rng.num()).collect();
    let tp = TblPr {
        tbl_w: if rng.one_in(2) {
            Some(WidthSpec { w: n(rng.num()), r#type: rng.pick(&["dxa", "pct", "auto"]).into(), ..WidthSpec::default() })
        } else {
            None
        },
        tbl_layout: if rng.one_in(3) { Some("fixed".into()) } else { None },
        bidi_visual: Some(rng.one_in(4)),
        tbl_ind: Some(dxa(rng.num())),
        jc: if rng.one_in(3) { Some(rng.pick(&["center", "right"]).into()) } else { None },
        tbl_cell_spacing: if rng.one_in(6) { Some(dxa(rng.num())) } else { None },
        ..all_borders()
    };
    table(id, rows, &grid, tp)
}

#[test]
fn no_panic_on_hostile_tables() {
    let doc = Document::default();
    let o = LayoutOptions {
        limits: LayoutLimits { max_table_cells: 100_000, max_depth: 40, ..LayoutLimits::default() },
        ..LayoutOptions::default()
    };
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut ok, mut limits, mut internal, mut rows) = (0usize, 0usize, 0usize, 0usize);
    let mut sites: IndexMap<String, usize> = IndexMap::new();
    for k in 0..600u32 {
        let t = hostile(&mut rng, k + 1, 3, k % 2 == 0);
        let frame = rng.num();
        with_cx(&doc, &o, |cx| {
            let mut host = Synth::new(&o);
            let at = LineRef { story: LineStory::Body, path: RefPath(vec![k].into()) };
            match layout_table(cx, &mut host, &t, rng.num(), frame, &at) {
                Ok(tl) => {
                    ok += 1;
                    for r in &tl.rows {
                        rows += 1;
                        assert_eq!(r.items.side.len(), r.items.list.items.len());
                        assert_eq!(r.border_items.side.len(), r.border_items.list.items.len());
                        if let Some(sp) = &r.split {
                            for avail in [rng.num(), 300.0] {
                                if let Ok(Some((a, b))) = sp.split(cx, avail) {
                                    assert_eq!(a.items.side.len(), a.items.list.items.len());
                                    let _ = at_page_top(&b);
                                }
                            }
                        }
                    }
                }
                Err(LayoutError::Limit(k)) => {
                    limits += 1;
                    *sites.entry(format!("{k:?}")).or_default() += 1;
                }
                Err(LayoutError::Internal(s)) => {
                    internal += 1;
                    *sites.entry(s.to_owned()).or_default() += 1;
                }
                Err(e) => panic!("{e:?}"),
            }
        });
    }
    eprintln!("hostile tables: {ok} laid out ({rows} rows), {limits} limits, {internal} TS throws: {sites:?}");
    assert!(ok > 200, "{ok}");
}

#[test]
fn limits_stop_explosions() {
    let doc = Document::default();
    let o = LayoutOptions {
        limits: LayoutLimits { max_table_cells: 10_000, max_depth: 8, ..LayoutLimits::default() },
        ..LayoutOptions::default()
    };
    // a gridSpan of 1e9: the grid would hold 1e9 columns (the TS allocates until memory runs out)
    let huge = table(1, vec![row(vec![cell("a", TcPr { grid_span: Some(n(1e9)), ..TcPr::default() })], None)], &[], TblPr::default());
    // 20,000 cells
    let many = table(2, (0..10_001).map(|_| row(vec![cell("", TcPr::default())], None)).collect(), &[1000.0], TblPr::default());
    // nesting deeper than the guard
    let mut deep = table(3, vec![row(vec![cell("x", TcPr::default())], None)], &[1000.0], TblPr::default());
    for k in 0..12 {
        let mut c = cell("y", TcPr::default());
        c.content.tables.push(TableAnchor { before_para: 0, table: Arc::new(deep) });
        deep = table(10 + k, vec![row(vec![c], None)], &[1000.0], TblPr::default());
    }
    for (t, want) in [(&huge, LimitKind::Table), (&many, LimitKind::Table), (&deep, LimitKind::Depth)] {
        let r = with_cx(&doc, &o, |cx| {
            let mut host = Synth::new(&o);
            let at = LineRef { story: LineStory::Body, path: RefPath(vec![0].into()) };
            layout_table(cx, &mut host, t, 0.0, 9000.0, &at).map(|t| t.rows.len())
        });
        assert_eq!(r, Err(LayoutError::Limit(want)), "table {}", t.id.0);
    }
}

#[test]
fn fixed_widths_and_cell_boxes() {
    let doc = Document::default();
    let o = LayoutOptions::default();
    let tc = |w: f64| TcPr { tc_w: Some(dxa(w)), ..TcPr::default() };
    let t = table(
        1,
        vec![row(vec![cell("a", tc(2000.0)), cell("b", tc(3000.0))], None)],
        &[2000.0, 3000.0],
        TblPr { tbl_layout: Some("fixed".into()), ..TblPr::default() },
    );
    let tl = with_cx(&doc, &o, |cx| {
        let mut host = Synth::new(&o);
        let at = LineRef { story: LineStory::Body, path: RefPath(vec![4].into()) };
        layout_table(cx, &mut host, &t, 1440.0, 9000.0, &at).unwrap()
    });
    assert_eq!(tl.width, 5000.0);
    let cells = tl.rows[0].cells.as_ref().unwrap();
    assert_eq!((cells[0].w, cells[1].w), (2000.0, 3000.0));
    // mode < 15 (the default settings): the text sits in the frame, the table starts a margin left
    assert_eq!(tl.x, 1440.0 - 108.0);
    assert_eq!(cells[0].at.to_ts(), "b4/r0/c0");
    assert_eq!(cells[1].at.to_ts(), "b4/r0/c1");
}
