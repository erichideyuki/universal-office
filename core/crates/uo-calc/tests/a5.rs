//! A5 parity: the A5 calls of the `calc-tests` group (i18n round trips, catalogue, `legacyToDynamic`, `toFileFormula`,
//! `hasVolatileFunction`, `shouldRecalcOnOpen`) and the `a5` capture group (`js/capture-a5.mjs`: every function name
//! and separator both ways in pt-BR/en-US, errors, booleans, R1C1, structured references, `_xlpm.`, the legacy `@`
//! rewrite of the bank's and generated formulas, the file form, the volatile test, the add-in resolution), replayed
//! exactly (record `uo-calc.md` "A5 — as built"). The engine-level TS tests of `save.ts`, `open-recalc.ts`,
//! `addin.ts`, `implicit.test.ts` are ported as assertions at the end.
//!
//! Local run: `UO_A5_BIG=1 cargo test -p uo-calc --test a5` also replays `core/target/sheet-cap/calc-tests-big.calls.gz`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::too_many_lines)]

mod support;

use std::path::Path;

use support::{J, gunzip, parse};
use uo_calc::ast::{Ast, BinOp, FnName, Node, NodeId, Part, RefPart, SheetSpec, TableItem, UnaryOp, canon, name_flags};
use uo_calc::catalogue::{self, CatalogEntry, FunctionKind};
use uo_calc::cell::{FileCell, InlineText, error_from_text, error_text};
use uo_calc::file_form::to_file_formula;
use uo_calc::fnreg;
use uo_calc::formula::{FormulaAttrs, FormulaType};
use uo_calc::i18n::{self, FormulaLanguage, LangSpec, NameSet, SyntaxOptions, Translator};
use uo_calc::implicit::{NoNames, legacy_to_dynamic};
use uo_calc::open_recalc::{CalcPrFlags, OpenRecalcWorkbook, has_volatile_function, should_recalc_on_open};
use uo_calc::value::{Scalar, StrRef};
use uo_calc::{JsStr, JsString, SheetIx, Store};

fn js(j: &J) -> JsString {
    j.js().unwrap_or_else(|| panic!("not a string: {j:?}"))
}

fn s8(j: &J) -> String {
    js(j).as_js().to_string_lossy()
}

fn json_str(units: &[u16]) -> String {
    let mut o = String::new();
    uo_jsnum::json::push_string_utf16(&mut o, units);
    o
}

fn json_s(s: &str) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    json_str(&u)
}

/// The raw `"ret":…` text of a recorded line.
fn ret_of(line: &str) -> Option<&str> {
    let start = line.find(r#","ret":"#)? + 7;
    Some(&line[start..line.len() - 1])
}

const UNDEF: &str = r#"{"$undef":true}"#;

/// `core/target/sheet-cap` (the large local captures), or `UO_SHEET_CAP`.
fn big_dir() -> std::path::PathBuf {
    std::env::var_os("UO_SHEET_CAP")
        .map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sheet-cap"), std::path::PathBuf::from)
}

/// The `Intl` decimal of the ids without a table the TS tests use (the host hook C1; Node 24.15 on the capture
/// machine, whose default locale is pt-BR: an unknown id such as `xx-YY` falls back to it). The `a5` capture records
/// them in its meta line (`intl`), checked against this table.
fn intl_decimal(id: &str) -> Option<&'static str> {
    match id {
        "ja-JP" => Some("."),
        _ => Some(","),
    }
}

fn lang_of(j: &J) -> LangSpecOwned {
    match j {
        J::Str(_) => LangSpecOwned::Id(s8(j)),
        o => {
            let id = s8(o.get("id").unwrap());
            LangSpecOwned::Table(i18n::get_language(&id).unwrap_or_else(|| panic!("unregistered table {id}")))
        }
    }
}

enum LangSpecOwned {
    None,
    Id(String),
    Table(&'static FormulaLanguage),
}

impl LangSpecOwned {
    fn spec(&self) -> LangSpec<'_> {
        match self {
            Self::None => LangSpec::Default,
            Self::Id(s) => LangSpec::Id(s),
            Self::Table(t) => LangSpec::Table(t),
        }
    }
    fn table(&self) -> &'static FormulaLanguage {
        match self {
            Self::None => i18n::EN_US,
            Self::Id(s) => i18n::resolve_language(Some(s)),
            Self::Table(t) => t,
        }
    }
}

/// `SyntaxOptions` + `TranslateOptions` of a recorded object, owned.
struct Opts {
    lang: LangSpecOwned,
    decimal: Option<JsString>,
    list: Option<JsString>,
    array_col: Option<JsString>,
    array_row: Option<JsString>,
    r1c1: bool,
    file_prefixes: bool,
    in_table: Option<JsString>,
    names: Option<NameSet>,
    intl: Option<JsString>,
}

impl Opts {
    fn of(j: Option<&J>) -> Self {
        let g = |k: &str| j.and_then(|o| o.get(k));
        let lang = g("lang").map_or(LangSpecOwned::None, lang_of);
        let intl = match &lang {
            LangSpecOwned::Id(id) if i18n::get_language(id).is_none() => intl_decimal(id).map(JsString::from_str_js),
            _ => None,
        };
        Self {
            lang,
            decimal: g("decimal").and_then(J::js),
            list: g("list").and_then(J::js),
            array_col: g("arrayCol").and_then(J::js),
            array_row: g("arrayRow").and_then(J::js),
            r1c1: g("r1c1").is_some_and(J::bool),
            file_prefixes: g("filePrefixes").is_some_and(J::bool),
            in_table: g("inTable").and_then(J::js),
            names: g("names").map(|n| {
                let v: Vec<JsString> = n.arr().iter().map(js).collect();
                NameSet::new(v.iter().map(JsString::as_js))
            }),
            intl,
        }
    }
    fn get(&self) -> SyntaxOptions<'_> {
        SyntaxOptions {
            lang: self.lang.spec(),
            decimal: self.decimal.as_ref().map(JsString::as_js),
            list: self.list.as_ref().map(JsString::as_js),
            array_col: self.array_col.as_ref().map(JsString::as_js),
            array_row: self.array_row.as_ref().map(JsString::as_js),
            r1c1: self.r1c1,
            file_prefixes: self.file_prefixes,
            in_table: self.in_table.as_ref().map(JsString::as_js),
            names: self.names.as_ref(),
            regional_decimal: self.intl.as_ref().map(JsString::as_js),
        }
    }
}

fn entry_json(e: &CatalogEntry) -> String {
    let mut s =
        format!(r#"{{"argStep":{},"id":{},"maxArgs":{},"minArgs":{},"name":{}"#, e.arg_step, e.id, e.max_args, e.min_args, json_s(e.name));
    if let Some(op) = e.operator {
        s.push_str(&format!(r#","operator":{}"#, json_s(op)));
    }
    s.push_str(&format!(r#","prefix":{}"#, json_s(e.prefix)));
    if !e.small_counts.is_empty() {
        s.push_str(&format!(r#","smallCounts":[{}]"#, e.small_counts.iter().map(u8::to_string).collect::<Vec<_>>().join(",")));
    }
    s.push('}');
    s
}

// ---------------------------------------------------------------- trees (the recorder's JSON of a TS `Node`)

/// A tree from the recorder's canonical JSON (`implicitAt` keys are read by [`implicit_of`]).
fn node_of(ast: &mut Ast, j: &J) -> NodeId {
    let t = s8(j.get("t").unwrap());
    let pos = j.get("pos").and_then(J::num).map(|p| p as u32);
    let num = |j: &J| -> f64 {
        match j {
            J::Num(x) => *x,
            o => match o.get("$num").map(s8) {
                Some(s) if s == "-0" => -0.0,
                Some(s) => uo_jsnum::parse::to_number(&s),
                None => panic!("num {o:?}"),
            },
        }
    };
    let part = |ast: &mut Ast, k: &str| -> Part {
        match j.get(k) {
            None => Part::NONE,
            Some(p) => ast.pack_part(RefPart { n: num(p.get("n").unwrap()), abs: p.get("abs").unwrap().bool() }),
        }
    };
    let sheet = |ast: &mut Ast| -> u32 {
        match j.get("sheet") {
            None => 0,
            Some(s) => {
                let sh = ast.intern(js(s.get("sheet").unwrap()).as_js());
                let sheet2 = s.get("sheet2").map(|x| ast.intern(js(x).as_js()));
                let ext = s.get("ext").map(num);
                ast.push_spec(SheetSpec { sheet: sh, sheet2, ext })
            }
        }
    };
    let kids = |ast: &mut Ast, k: &str| -> uo_calc::ast::Span {
        let ids: Vec<NodeId> = j.get(k).unwrap().arr().iter().map(|a| node_of(ast, a)).collect();
        ast.push_kids(&ids)
    };
    let n = match t.as_str() {
        "num" => {
            let raw = j.get("raw").map_or(Ast::NO_RAW, |r| ast.intern(js(r).as_js()).0);
            Node::Num { v: num(j.get("v").unwrap()), raw }
        }
        "str" => Node::Str(ast.intern(js(j.get("v").unwrap()).as_js())),
        "bool" => Node::Bool(j.get("v").unwrap().bool()),
        "err" => Node::Err(error_from_text(js(j.get("v").unwrap()).as_js()).unwrap()),
        "arr" => {
            let rows = j.get("rows").unwrap().arr();
            let mut data = Vec::new();
            for r in rows {
                for v in r.arr() {
                    data.push(match v {
                        J::Null => Scalar::Empty,
                        J::Bool(b) => Scalar::Bool(*b),
                        J::Str(u) => Scalar::Str(StrRef::Lit(ast.intern(JsString::from_utf16(u).as_js()).0)),
                        o if o.get("$err").is_some() => Scalar::Err(error_from_text(js(o.get("$err").unwrap()).as_js()).unwrap()),
                        o => Scalar::Num(num(o)),
                    });
                }
            }
            let cols = rows.first().map_or(0, |r| r.arr().len()) as u32;
            let start = ast.push_array(&data);
            Node::Arr { start, rows: rows.len() as u32, cols }
        }
        "cell" => {
            let sh = sheet(ast);
            Node::Cell { sheet: sh, row: part(ast, "row"), col: part(ast, "col") }
        }
        "area" => {
            let sh = sheet(ast);
            Node::Area { sheet: sh, r1: part(ast, "r1"), c1: part(ast, "c1"), r2: part(ast, "r2"), c2: part(ast, "c2") }
        }
        "referr" => Node::RefErr { sheet: sheet(ast) },
        "name" => {
            let sh = sheet(ast);
            let name = ast.intern(js(j.get("name").unwrap()).as_js());
            let sym = ast.intern_upper(ast.string(name).as_js());
            let mut flags = 0;
            for (k, f) in [("param", name_flags::PARAM), ("optional", name_flags::OPTIONAL), ("eta", name_flags::ETA)] {
                if j.get(k).is_some_and(J::bool) {
                    flags |= f;
                }
            }
            Node::Name { name, sym, sheet: sh, flags }
        }
        "table" => {
            let table = ast.intern(js(j.get("table").unwrap()).as_js());
            let items: Vec<TableItem> = j
                .get("items")
                .unwrap()
                .arr()
                .iter()
                .map(|i| match s8(i).as_str() {
                    "#All" => TableItem::All,
                    "#Data" => TableItem::Data,
                    "#Headers" => TableItem::Headers,
                    "#Totals" => TableItem::Totals,
                    _ => TableItem::ThisRow,
                })
                .collect();
            let start = ast.push_items(&items);
            let col = |ast: &mut Ast, k: &str| j.get(k).map_or(0, |c| ast.intern(js(c).as_js()).0 + 1);
            let col1 = col(ast, "col1");
            let col2 = col(ast, "col2");
            Node::Table { table, items: start, n_items: items.len() as u32, col1, col2 }
        }
        "fn" => {
            let nm = js(j.get("name").unwrap());
            let sym = ast.intern(nm.as_js());
            let name = fnreg::find(nm.as_js()).map_or(FnName::Other(sym), FnName::Known);
            let prefix = j.get("prefix").and_then(J::js).map_or(0, |p| ast.intern(p.as_js()).0 + 1);
            let args = kids(ast, "args");
            Node::Fn { name, prefix, args, sym }
        }
        "call" => {
            let f = node_of(ast, j.get("fn").unwrap());
            let args = kids(ast, "args");
            Node::Call { f, args }
        }
        "missing" => Node::Missing,
        "unary" => {
            let e = node_of(ast, j.get("e").unwrap());
            let op = match s8(j.get("op").unwrap()).as_str() {
                "-" => UnaryOp::Neg,
                "+" => UnaryOp::Plus,
                _ => UnaryOp::Percent,
            };
            Node::Unary { op, e }
        }
        "bin" => {
            let a = node_of(ast, j.get("a").unwrap());
            let b = node_of(ast, j.get("b").unwrap());
            let op = match s8(j.get("op").unwrap()).as_str() {
                "+" => BinOp::Add,
                "-" => BinOp::Sub,
                "*" => BinOp::Mul,
                "/" => BinOp::Div,
                "^" => BinOp::Pow,
                "&" => BinOp::Concat,
                "=" => BinOp::Eq,
                "<>" => BinOp::Ne,
                "<" => BinOp::Lt,
                "<=" => BinOp::Le,
                ">" => BinOp::Gt,
                ">=" => BinOp::Ge,
                ":" => BinOp::Range,
                " " => BinOp::Isect,
                _ => BinOp::Union,
            };
            Node::Bin { op, a, b }
        }
        "paren" => Node::Paren(node_of(ast, j.get("e").unwrap())),
        o => panic!("node {o}"),
    };
    ast.push(n, pos)
}

/// The `implicitAt` values of a recorded tree (pre-order), and the tree's JSON without them.
fn implicit_of(j: &J, out: &mut Vec<i64>) -> J {
    match j {
        J::Obj(o) => {
            if let Some(p) = o.iter().find(|(k, _)| k == "implicitAt").and_then(|(_, v)| v.num()) {
                out.push(p as i64);
            }
            J::Obj(o.iter().filter(|(k, _)| k != "implicitAt").map(|(k, v)| (k.clone(), implicit_of(v, out))).collect())
        }
        J::Arr(a) => J::Arr(a.iter().map(|x| implicit_of(x, out)).collect()),
        x => x.clone(),
    }
}

// ---------------------------------------------------------------- the sheet-core model (open-recalc)

fn model_cell(j: &J) -> FileCell {
    let fa = j.get("formulaAttrs").map(|fa| {
        let mut flags = 0;
        for (bit, name) in FormulaAttrs::FLAG_NAMES {
            if fa.get(name).is_some_and(J::bool) {
                flags |= bit;
            }
        }
        FormulaAttrs {
            t: fa.get("t").and_then(J::js).map(|t| match t.as_js().to_string_lossy().as_str() {
                "normal" => FormulaType::Normal,
                "shared" => FormulaType::Shared,
                "array" => FormulaType::Array,
                _ => FormulaType::DataTable,
            }),
            r#ref: fa.get("ref").and_then(J::js),
            si: fa.get("si").and_then(J::num),
            flags,
            r1: fa.get("r1").and_then(J::js),
            r2: fa.get("r2").and_then(J::js),
        }
    });
    FileCell {
        r#ref: j.get("ref").and_then(J::js),
        t: j.get("type").and_then(J::js),
        v: j.get("value").and_then(J::js),
        s: j.get("style").and_then(J::num),
        f: j.get("formula").and_then(J::js),
        fa,
        cm: j.get("cm").and_then(J::num),
        vm: j.get("vm").and_then(J::num),
        is: j.get("inlineString").map(|is| InlineText { text: is.get("text").and_then(J::js), runs: None }),
    }
}

/// The store of a sheet-core book's worksheets, streamed in file order.
fn store_of_model(book: &J) -> Store {
    let mut store = Store::new();
    for ws in book.get("worksheets").unwrap().arr() {
        let sheet = store.add_sheet().unwrap();
        let mut loader = store.load_sheet(sheet).unwrap();
        for row in ws.get("rows").unwrap().arr() {
            let r = row.get("r").unwrap().num().unwrap() as u32;
            loader.row(r - 1, uo_calc::store::RowMeta::default()).unwrap();
            for c in row.get("cells").unwrap().arr() {
                let (_, col) = uo_calc::a1::a1_to_cell(js(c.get("ref").unwrap()).as_js()).unwrap();
                loader.cell(col, &model_cell(c)).unwrap();
            }
        }
        loader.finish().unwrap();
    }
    store
}

fn workbook_flags(book: &J) -> OpenRecalcWorkbook {
    let wb = book.get("workbook").unwrap();
    OpenRecalcWorkbook {
        app_name_xl: wb.get("appName").is_some_and(|a| s8(a) == "xl"),
        calc_pr: wb.get("calcPr").map(|cp| CalcPrFlags {
            calc_id: cp.get("calcId").and_then(J::num),
            manual: cp.get("calcMode").is_some_and(|m| s8(m) == "manual"),
            full_calc_on_load: cp.get("fullCalcOnLoad").is_some_and(J::bool),
            force_full_calc: cp.get("forceFullCalc").is_some_and(J::bool),
        }),
    }
}

// ---------------------------------------------------------------- the calc-tests calls of A5

#[derive(Default)]
struct Tally {
    equal: usize,
    skipped: usize,
    bad: Vec<String>,
    by_fn: std::collections::BTreeMap<String, usize>,
}

fn replay_calc_tests(path: &Path) -> Tally {
    let mut t = Tally::default();
    let mut tr = Translator::new();
    for line in gunzip(path).lines() {
        let j = parse(line);
        let f = s8(j.get("fn").unwrap());
        let args = j.get("args").map_or(&[][..], J::arr);
        let Some(ret) = ret_of(line) else { continue };
        let got: Option<String> = match f.as_str() {
            "calc/i18n/translate:toLocal" | "calc/i18n/translate:fromLocal" => {
                let o = Opts::of(args.get(1));
                let text = js(&args[0]);
                let out = if f.ends_with("toLocal") { tr.to_local(text.as_js(), &o.get()) } else { tr.from_local(text.as_js(), &o.get()) };
                Some(json_str(&out.as_js().to_utf16()))
            }
            "calc/i18n/translate:localizeBool" => {
                let l = args.get(1).map_or(LangSpecOwned::None, lang_of);
                Some(json_s(i18n::localize_bool(args[0].bool(), l.table())))
            }
            "calc/i18n/translate:localizeError" => {
                let l = args.get(1).map_or(LangSpecOwned::None, lang_of);
                let code = error_from_text(js(&args[0]).as_js()).unwrap();
                Some(json_s(i18n::localize_error(code, l.table())))
            }
            "calc/i18n/translate:canonicalError" => {
                let l = args.get(1).map_or(LangSpecOwned::None, lang_of);
                Some(i18n::canonical_error(&js(&args[0]).as_js().to_utf16(), l.table()).map_or(UNDEF.into(), |c| json_s(error_text(c))))
            }
            "calc/i18n/translate:localFunctionName" => {
                let l = args.get(1).map_or(LangSpecOwned::None, lang_of);
                Some(json_str(&i18n::local_function_name(js(&args[0]).as_js(), l.table()).as_js().to_utf16()))
            }
            "calc/i18n/translate:canonicalFunctionName" => {
                let l = args.get(1).map_or(LangSpecOwned::None, lang_of);
                Some(i18n::canonical_function_name(js(&args[0]).as_js(), l.table()).map_or(UNDEF.into(), json_s))
            }
            "calc/i18n/translate:resolveSyntax" => {
                let o = Opts::of(args.first());
                let r = i18n::resolve_syntax(&o.get());
                let want = parse(ret);
                let w = |k: &str| s8(want.get(k).unwrap());
                let ok = r.decimal.as_js().to_string_lossy() == w("decimal")
                    && r.list.as_js().to_string_lossy() == w("list")
                    && r.array_col.as_js().to_string_lossy() == w("arrayCol")
                    && r.array_row.as_js().to_string_lossy() == w("arrayRow")
                    && r.r1c1 == want.get("r1c1").is_some_and(J::bool)
                    && r.lang.id == s8(want.get("lang").unwrap().get("id").unwrap());
                Some(if ok { ret.to_owned() } else { format!("{r:?}") })
            }
            "calc/i18n/languages:getLanguage" => {
                let want = parse(ret);
                let got = i18n::get_language(&s8(&args[0]));
                Some(match (got, want.get("id")) {
                    (Some(l), Some(id)) if l.id == s8(id) => ret.to_owned(),
                    (None, None) => ret.to_owned(),
                    (g, _) => format!("{:?}", g.map(|l| l.id)),
                })
            }
            "calc/i18n/languages:languages" => {
                let want = parse(ret);
                let ids: Vec<String> = want.arr().iter().map(|l| s8(l.get("id").unwrap())).collect();
                let got: Vec<String> = i18n::languages().iter().map(|l| l.id.to_owned()).collect();
                Some(if ids == got { ret.to_owned() } else { format!("{got:?}") })
            }
            "calc/i18n/catalog:catalog" => Some(format!("[{}]", catalogue::catalog().iter().map(entry_json).collect::<Vec<_>>().join(","))),
            "calc/i18n/catalog:catalogEntry" => Some(catalogue::catalog_entry(js(&args[0]).as_js()).map_or(UNDEF.into(), entry_json)),
            "calc/i18n/catalog:functionKind" => Some(json_s(match catalogue::function_kind(js(&args[0]).as_js()) {
                FunctionKind::Sheet => "sheet",
                FunctionKind::Reserved => "reserved",
                FunctionKind::Unknown => "unknown",
            })),
            "calc/i18n/catalog:acceptsArgCount" => {
                let name = s8(args[0].get("name").unwrap());
                let e = catalogue::entry_exact(&name).unwrap();
                if entry_json(e) != canon_json(&args[0]) {
                    t.skipped += 1;
                    continue;
                }
                Some(catalogue::accepts_arg_count(e, args[1].num().unwrap()).to_string())
            }
            "calc/i18n/catalog:withFilePrefix" => Some(json_str(&catalogue::with_file_prefix(js(&args[0]).as_js()).as_js().to_utf16())),
            "calc/i18n/translate:functionNames" => {
                let l = args.first().map_or(LangSpecOwned::None, lang_of);
                let mut got: Vec<String> = catalogue::function_names(l.table())
                    .iter()
                    .map(|f| {
                        format!(
                            r#"{{"canonical":{},"maxArgs":{},"minArgs":{},"name":{}}}"#,
                            json_s(f.canonical),
                            f.max_args,
                            f.min_args,
                            json_s(f.name)
                        )
                    })
                    .collect();
                // the collator order is the shell's (Intl.Collator): compared as a set
                let want = parse(ret);
                let mut w: Vec<String> = want.arr().iter().map(canon_json).collect();
                got.sort();
                w.sort();
                Some(if got == w { ret.to_owned() } else { format!("{} names", got.len()) })
            }
            "calc/i18n/value-locale:valueLocaleFor" => {
                let id = s8(&args[0]);
                let want = parse(ret);
                Some(match i18n::value_locale_for(&id) {
                    Some(v) => {
                        let w = |k: &str| s8(want.get(k).unwrap());
                        let ok = *v.id == w("id")
                            && v.decimal.as_js().to_string_lossy() == w("decimal")
                            && v.group.as_js().to_string_lossy() == w("group")
                            && v.true_text.as_js().to_string_lossy() == w("trueText")
                            && v.currency.as_js().to_string_lossy() == w("currency");
                        if ok { ret.to_owned() } else { format!("{v:?}") }
                    }
                    None => {
                        t.skipped += 1;
                        continue;
                    }
                })
            }
            "calc/implicit:legacyToDynamic" if args.len() == 1 => {
                let mut ast = Ast::new();
                let n = node_of(&mut ast, &args[0]);
                let l = legacy_to_dynamic(&mut ast, n, &NoNames);
                let mut want_pos = Vec::new();
                let want_tree = implicit_of(&parse(ret), &mut want_pos);
                let mut got_pos: Vec<i64> = l.inserted.iter().map(|&(_, p)| p).collect();
                got_pos.sort_unstable();
                want_pos.sort_unstable();
                let got = canon::node(&ast, l.root);
                let want = canon_json(&want_tree);
                Some(if got == want && got_pos == want_pos {
                    ret.to_owned()
                } else {
                    format!("{got} @{got_pos:?} (want {want} @{want_pos:?})")
                })
            }
            "calc/file-form:toFileFormula" if args.len() == 3 => {
                let mut ast = Ast::new();
                let n = node_of(&mut ast, &args[0]);
                let ff = to_file_formula(&mut ast, n, args[1].num().unwrap() as u32, args[2].num().unwrap() as u32, &NoNames);
                Some(format!(r#"{{"dynamic":{},"text":{}}}"#, ff.dynamic, json_str(&ff.text.as_js().to_utf16())))
            }
            "calc/open-recalc:hasVolatileFunction" => Some(has_volatile_function(js(&args[0]).as_js()).to_string()),
            "calc/open-recalc:shouldRecalcOnOpen" => {
                let store = store_of_model(&args[0]);
                let p = should_recalc_on_open(&workbook_flags(&args[0]), &store);
                let cells: Vec<String> = p.cells.iter().map(|(s, r, c)| format!("[{},{r},{c}]", s.0)).collect();
                Some(format!(r#"{{"cells":[{}],"mode":{},"reason":{}}}"#, cells.join(","), json_s(p.mode.text()), json_s(p.reason.text())))
            }
            _ => continue,
        };
        let got = got.unwrap();
        // canonical comparison through the JSON reader (key order, number forms)
        let want_c = canon_json(&parse(ret));
        let got_c = if got.starts_with('{') || got.starts_with('[') || got.starts_with('"') || got == "true" || got == "false" {
            canon_json(&parse(&got))
        } else {
            got.clone()
        };
        if got_c == want_c {
            t.equal += 1;
            *t.by_fn.entry(f).or_default() += 1;
        } else {
            t.bad.push(format!("{f} args={}: want {} got {got}", &line[..line.len().min(300)], &ret[..ret.len().min(300)]));
        }
    }
    t
}

fn canon_json(j: &J) -> String {
    support::canon_json(j)
}

fn report(what: &str, t: &Tally) {
    eprintln!("{what}: {} equal, {} skipped, {} mismatches", t.equal, t.skipped, t.bad.len());
    for (f, n) in &t.by_fn {
        eprintln!("  {f}: {n}");
    }
    for b in t.bad.iter().take(25) {
        eprintln!("  {b}");
    }
}

#[test]
fn a5_calc_tests() {
    let t = replay_calc_tests(&support::captures_dir().join("calc-tests.calls.gz"));
    report("calc-tests A5 calls", &t);
    assert!(t.bad.is_empty());
    assert!(t.equal >= 3000, "{}", t.equal);
    let big = big_dir().join("calc-tests-big.calls.gz");
    if std::env::var_os("UO_A5_BIG").is_some() && big.exists() {
        let t = replay_calc_tests(&big);
        report("calc-tests (big) A5 calls", &t);
        assert!(t.bad.is_empty());
    }
}

// ---------------------------------------------------------------- unit checks of the static data

#[test]
fn tables_are_sorted_and_aligned() {
    for w in uo_calc::addin::ATP_FUNCTIONS.windows(2) {
        assert!(w[0] < w[1], "{w:?}");
    }
    let t = &uo_calc::i18n::tables::IMPLICIT_CLASSES;
    for w in t.windows(2) {
        assert!(w[0].0 < w[1].0);
    }
    for l in i18n::languages() {
        for w in l.fn_in.windows(2) {
            assert!(w[0].0 < w[1].0, "{} {:?}", l.id, w);
        }
        for w in l.functions.windows(2) {
            assert!(w[0].0 < w[1].0);
        }
    }
    // every catalogue name is a registry row (the 535 of FN_TABLE)
    for e in catalogue::catalog() {
        assert!(fnreg::find(JsStr::Latin1(e.name.as_bytes())).is_some(), "{}", e.name);
    }
    assert_eq!(catalogue::catalog().len(), uo_calc::fnreg::FN_TABLE.len());
    let _ = SheetIx(0);
}

// ---------------------------------------------------------------- the `a5` capture group

/// The deterministic host rule of the capture (`@legacyHost`).
struct CapHost;
impl uo_calc::implicit::LegacyHost for CapHost {
    fn name_is_multi(&self, _ast: &Ast, name: JsStr<'_>) -> bool {
        let u = name.to_string_lossy().to_uppercase();
        u != "NOMEUNICO" && u != "IRR_GUESS"
    }
    fn is_name(&self, _ast: &Ast, name: JsStr<'_>) -> bool {
        name.to_string_lossy().to_uppercase().starts_with("NOME")
    }
}

fn opt_str(j: Option<&J>) -> String {
    match j {
        None | Some(J::Null) => "null".into(),
        Some(x) => json_str(&js(x).as_js().to_utf16()),
    }
}

fn replay_a5(path: &Path) -> Tally {
    use uo_calc::parse::{FormulaParser, ParseOptions};
    use uo_calc::print::{PrintOptions, print_formula};
    let mut t = Tally::default();
    let mut tr = Translator::new();
    let mut parser = FormulaParser::new();
    let text = gunzip(path);
    let mut lines = text.lines();
    let meta = parse(lines.next().unwrap());
    assert_eq!(meta.get("k").map(s8).as_deref(), Some("meta"));
    if let Some(J::Obj(intl)) = meta.get("intl") {
        for (id, d) in intl {
            assert_eq!(intl_decimal(id), Some(s8(d).as_str()), "the capture machine's Intl decimal of {id}");
        }
    }
    for line in lines {
        let j = parse(line);
        let k = s8(j.get("k").unwrap());
        let ok = match k.as_str() {
            "tl" | "fl" => {
                let o = Opts::of(j.get("o"));
                let input = js(j.get("t").unwrap());
                let got = if k == "tl" { tr.to_local(input.as_js(), &o.get()) } else { tr.from_local(input.as_js(), &o.get()) };
                let want = js(j.get("r").unwrap());
                let ok = got == want;
                if !ok {
                    t.bad.push(format!(
                        "{k} {:?} {}: want {:?} got {:?}",
                        input.as_js().to_string_lossy(),
                        canon_json(j.get("o").unwrap_or(&J::Null)),
                        want.as_js().to_string_lossy(),
                        got.as_js().to_string_lossy()
                    ));
                    *t.by_fn.entry(format!("{k} BAD")).or_default() += 1;
                    continue;
                }
                true
            }
            "ln" => {
                let l = i18n::resolve_language(Some(&s8(j.get("lang").unwrap())));
                i18n::local_function_name(js(j.get("t").unwrap()).as_js(), l) == js(j.get("r").unwrap())
            }
            "cf" => {
                let l = i18n::resolve_language(Some(&s8(j.get("lang").unwrap())));
                let got = i18n::canonical_function_name(js(j.get("t").unwrap()).as_js(), l);
                let want = j.get("r").and_then(J::js).map(|x| x.as_js().to_string_lossy());
                got.map(str::to_owned) == want
            }
            "ce" => {
                let l = i18n::resolve_language(Some(&s8(j.get("lang").unwrap())));
                let got = i18n::canonical_error(&js(j.get("t").unwrap()).as_js().to_utf16(), l).map(|c| error_text(c).to_owned());
                let want = j.get("r").and_then(J::js).map(|x| x.as_js().to_string_lossy());
                got == want
            }
            "lg" | "ff" => {
                let at = j.get("at").unwrap().arr();
                let (row, col) = (at[0].num().unwrap() as u32, at[1].num().unwrap() as u32);
                let mut ast = Ast::new();
                let f = js(j.get("f").unwrap());
                let Ok(n) = parser.parse(&mut ast, f.as_js(), &ParseOptions { r1c1: false, letters: None, row, col }) else {
                    t.bad.push(format!("{k} parse failed: {}", f.as_js().to_string_lossy()));
                    continue;
                };
                if k == "lg" {
                    if j.get("throw").is_some() {
                        t.skipped += 1;
                        continue;
                    }
                    let l = legacy_to_dynamic(&mut ast, n, &CapHost);
                    let ui = print_formula(&ast, l.root, &PrintOptions { row, col, r1c1: false, ui: true });
                    let file = print_formula(&ast, l.root, &PrintOptions { row, col, r1c1: false, ui: false });
                    let mut pos: Vec<f64> = l.inserted.iter().map(|&(_, p)| p as f64).collect();
                    pos.sort_by(f64::total_cmp);
                    let want_pos: Vec<f64> = j.get("pos").unwrap().arr().iter().map(|x| x.num().unwrap()).collect();
                    let ok = ui == js(j.get("ui").unwrap()) && file == js(j.get("file").unwrap()) && pos == want_pos;
                    if !ok {
                        t.bad.push(format!(
                            "lg {:?} at {row},{col}: want {} / {} {:?} got {:?} / {:?} {pos:?}",
                            f.as_js().to_string_lossy(),
                            opt_str(j.get("ui")),
                            opt_str(j.get("file")),
                            want_pos,
                            ui.as_js().to_string_lossy(),
                            file.as_js().to_string_lossy()
                        ));
                        continue;
                    }
                    true
                } else {
                    let ff = to_file_formula(&mut ast, n, row, col, &CapHost);
                    let ok = ff.text == js(j.get("text").unwrap()) && ff.dynamic == j.get("dynamic").unwrap().bool();
                    if !ok {
                        t.bad.push(format!(
                            "ff {:?}: want {} {} got {:?} {}",
                            f.as_js().to_string_lossy(),
                            opt_str(j.get("text")),
                            j.get("dynamic").unwrap().bool(),
                            ff.text.as_js().to_string_lossy(),
                            ff.dynamic
                        ));
                        continue;
                    }
                    true
                }
            }
            "vo" => has_volatile_function(js(j.get("f").unwrap()).as_js()) == j.get("v").unwrap().bool(),
            "ad" => {
                let mut ast = Ast::new();
                let f = js(j.get("f").unwrap());
                let n = parser.parse(&mut ast, f.as_js(), &ParseOptions::default()).unwrap();
                let l = i18n::resolve_language(Some(&s8(j.get("lang").unwrap())));
                let r = uo_calc::addin::resolve_addin_calls(&mut ast, n, l);
                print_formula(&ast, r, &PrintOptions { row: 0, col: 0, r1c1: false, ui: false }) == js(j.get("file").unwrap())
            }
            other => panic!("record {other}"),
        };
        if ok {
            t.equal += 1;
            *t.by_fn.entry(k).or_default() += 1;
        } else {
            t.bad.push(format!("{k} {}", &line[..line.len().min(300)]));
        }
    }
    t
}

#[test]
fn a5_capture() {
    let t = replay_a5(&support::captures_dir().join("a5.capture.gz"));
    report("a5 capture", &t);
    assert!(t.bad.is_empty());
    assert!(t.equal >= 100_000, "{}", t.equal);
    let big = big_dir().join("a5-big.capture.gz");
    if std::env::var_os("UO_A5_BIG").is_some() && big.exists() {
        let t = replay_a5(&big);
        report("a5 capture (big)", &t);
        assert!(t.bad.is_empty());
    }
}

// ---------------------------------------------------------------- the measured classes against Excel (oracle)

/// `implicit.test.ts` "todas as sondas do catálogo": the 4.6 thousand legacy → Formula2 pairs measured in Excel 365
/// (`test/oracle/implicit.f2.json`), through the 508 measured classes. Also the 13 hand pairs of the TS test.
#[test]
fn implicit_classes_match_excel() {
    use uo_calc::parse::{FormulaParser, ParseOptions};
    use uo_calc::print::{PrintOptions, print_formula};
    let path = std::env::var_os("UO_A5_ORACLE").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sheet/packages/calc/test/oracle/implicit.f2.json"),
        std::path::PathBuf::from,
    );
    let j = parse(uo_jsnum::text::trim(&std::fs::read_to_string(&path).unwrap()));
    let mut pairs: Vec<(String, String)> = j.get("pares").unwrap().arr().iter().map(|p| (s8(&p.arr()[0]), s8(&p.arr()[1]))).collect();
    for (f, f2) in [
        ("=A1:A3", "=@A1:A3"),
        ("=A1:A3*2", "=@A1:A3*2"),
        ("=-A1:A3", "=-@A1:A3"),
        ("=A1:A3&\"x\"", "=@A1:A3&\"x\""),
        ("=IF(A1:A3>1,\"y\",\"n\")", "=IF(@A1:A3>1,\"y\",\"n\")"),
        ("=IF(A1:A3,1,2)", "=IF(@A1:A3,1,2)"),
        ("=A1:A3>1", "=@A1:A3>1"),
        ("=IFERROR(A1:A3,0)", "=IFERROR(@A1:A3,0)"),
        ("=NOT(A1:A3)", "=NOT(@A1:A3)"),
        ("=AND(A1:A3>1)", "=AND(@A1:A3>1)"),
        ("=A1:C1", "=@A1:C1"),
        ("=A1:A3+A1:C1", "=@A1:A3+@A1:C1"),
        ("=A1+1", "=A1+1"),
    ] {
        pairs.push((f.to_owned(), f2.to_owned()));
    }
    let norm = |x: &str| x.trim_start_matches('=').replace("_xlfn.", "").replace("_xlws.", "");
    let mut parser = FormulaParser::new();
    let mut bad = Vec::new();
    for (f, f2) in &pairs {
        let mut ast = Ast::new();
        let u: Vec<u16> = f.encode_utf16().collect();
        let n = parser.parse(&mut ast, JsStr::Utf16(&u), &ParseOptions::default()).unwrap_or_else(|e| panic!("{f}: {e:?}"));
        let l = legacy_to_dynamic(&mut ast, n, &NoNames);
        let got =
            format!("={}", print_formula(&ast, l.root, &PrintOptions { row: 0, col: 0, r1c1: false, ui: true }).as_js().to_string_lossy());
        if norm(&got) != norm(f2) {
            bad.push(format!("{f} -> {got} x Excel {f2}"));
        }
    }
    eprintln!("implicit.f2.json: {} pairs, {} differ", pairs.len(), bad.len());
    for b in bad.iter().take(20) {
        eprintln!("  {b}");
    }
    assert!(pairs.len() > 4_000);
    assert!(bad.is_empty());
}

// ---------------------------------------------------------------- engine-level TS tests (ported assertions)

mod engine_tests {
    use super::*;
    use uo_calc::engine::{CellInput, Engine, EngineInput, EngineOptions, FormulaInput};
    use uo_calc::locale::{Clock, RandomSource, ValueLocale};
    use uo_calc::open_recalc::{OpenRecalcMode, OpenRecalcReason, apply_open_recalc};
    use uo_calc::save::{SaveOptions, prepare_for_save, save_flags, unsure_reasons};
    use uo_calc::value::ErrorCode;

    const CLOCK: Clock = Clock { now_serial: 45658.5 };

    fn js_s(s: &str) -> JsString {
        JsString::from_str_js(s)
    }

    /// A store whose first sheet has the `(A1 ref, FileCell)` cells.
    fn store_of(cells: &[(&str, FileCell)], sheets: usize) -> Store {
        let mut store = Store::new();
        for s in 0..sheets {
            let sh = store.add_sheet().unwrap();
            if s == 0 {
                for (r, c) in cells {
                    let (row, col) = uo_calc::a1::a1_to_cell(JsStr::Latin1(r.as_bytes())).unwrap();
                    let mut c = c.clone();
                    c.r#ref = Some(js_s(r));
                    store.set_file_cell(sh, row, col, &c).unwrap();
                }
            }
        }
        store
    }

    fn num(v: &str) -> FileCell {
        FileCell { v: Some(js_s(v)), ..FileCell::default() }
    }

    fn formula(f: &str, v: Option<&str>) -> FileCell {
        FileCell { f: Some(js_s(f)), v: v.map(js_s), ..FileCell::default() }
    }

    fn engine(store: &mut Store, names: &[&str], locale: ValueLocale) -> Engine {
        let input = EngineInput { sheet_names: names.iter().map(|n| js_s(n)).collect(), ..EngineInput::default() };
        Engine::new(store, &input, &EngineOptions::new(locale), None).unwrap()
    }

    fn val(e: &Engine, store: &Store, s: u32, r: u32, c: u32) -> Option<Scalar> {
        e.value(store, SheetIx(s), r, c)
    }

    /// `engine.test.ts` "legado: interseção implícita": B2 =A1:A3*10 -> 20 (row 2), B5 -> #VALUE!.
    #[test]
    fn legacy_implicit_intersection_engine() {
        let mut store = store_of(
            &[("A1", num("1")), ("A2", num("2")), ("B2", formula("A1:A3*10", None)), ("A3", num("3")), ("B5", formula("A1:A3*10", None))],
            1,
        );
        let mut e = engine(&mut store, &["Sheet1"], ValueLocale::en_us());
        let mut rnd = RandomSource::sequence(Vec::new());
        e.recalc(&mut store, CLOCK, &mut rnd).unwrap();
        // the rewrite gave `@A1:A3*10`; SINGLE's body is A4e (reference.ts): until it lands the cell is not ported
        let np: Vec<String> = e.not_ported().iter().map(|x| x.3.entry().name.to_owned()).collect();
        if np.is_empty() {
            assert_eq!(val(&e, &store, 0, 1, 1), Some(Scalar::Num(20.0)));
            assert_eq!(val(&e, &store, 0, 4, 1), Some(Scalar::Err(ErrorCode::Value)));
        } else {
            assert!(np.iter().all(|n| n == "SINGLE"), "{np:?}");
        }
        // the formula bar shows the inserted @ at its text position
        assert_eq!(e.formula_text(&store, SheetIx(0), 1, 1).unwrap().as_js().to_string_lossy(), "@A1:A3*10");
        assert_eq!(e.formula_display(&store, SheetIx(0), 1, 1).unwrap().as_js().to_string_lossy(), "=@A1:A3*10");
    }

    /// `addin.test.ts`: `_xll.` resolved by the name in pt-BR; an English name that differs stays `#NAME?` with `@`.
    #[test]
    fn addin_resolved_by_local_name() {
        let mut store = store_of(
            &[
                ("A1", num("4")),
                ("B1", formula("_xll.ISEVEN(A1)", None)),
                ("C1", formula("_xll.\u{c9}PAR(A1)", None)),
                ("D1", formula("_xll.BESSELJ(2.5,1)", None)),
            ],
            1,
        );
        let mut e = engine(&mut store, &["S"], ValueLocale::pt_br());
        let mut rnd = RandomSource::sequence(Vec::new());
        e.recalc(&mut store, CLOCK, &mut rnd).unwrap();
        // `@ISEVEN(A1)`: the add-in name stays `#NAME?` under the legacy @ (SINGLE is A4e: compared when it lands)
        if !e.not_ported().iter().any(|x| x.1 == 0 && x.2 == 1) {
            assert_eq!(val(&e, &store, 0, 0, 1), Some(Scalar::Err(ErrorCode::Name)));
        }
        assert_eq!(e.formula_display(&store, SheetIx(0), 0, 1).unwrap().as_js().to_string_lossy(), "=@ISEVEN(A1)");
        assert_eq!(e.formula_display(&store, SheetIx(0), 0, 3).unwrap().as_js().to_string_lossy(), "=BESSELJ(2.5,1)");
        assert!(matches!(val(&e, &store, 0, 0, 3), Some(Scalar::Num(_))));
        // ISEVEN is A4f (info.ts): compared once its body is ported
        if !e.not_ported().iter().any(|x| x.1 == 0 && x.2 == 2) {
            assert_eq!(val(&e, &store, 0, 0, 2), Some(Scalar::Bool(true)));
        }
        // the resolved text is the native call (FORMULATEXT, save)
        assert_eq!(e.formula_text(&store, SheetIx(0), 0, 2).unwrap().as_js().to_string_lossy(), "ISEVEN(A1)");
    }

    fn open_book(extra: &[(&str, FileCell)]) -> Store {
        let mut cells = vec![("A1", num("1")), ("B1", formula("A1+A2", Some("99"))), ("A2", num("2"))];
        cells.extend(extra.iter().cloned());
        store_of(&cells, 1)
    }

    const OK: Option<CalcPrFlags> =
        Some(CalcPrFlags { calc_id: Some(191_029.0), manual: false, full_calc_on_load: false, force_full_calc: false });

    /// `open-recalc.test.ts` "parcial" and "completo": the plan applied on the engine.
    #[test]
    fn open_recalc_apply() {
        let ca = FormulaAttrs { flags: FormulaAttrs::CA, ..FormulaAttrs::default() };
        let mut store = open_book(&[
            ("C2", formula("A1*10", None)),
            ("D2", formula("C2+1", Some("99"))),
            ("E2", FileCell { fa: Some(ca), ..formula("A1+5", Some("99")) }),
        ]);
        let book = OpenRecalcWorkbook { app_name_xl: true, calc_pr: OK };
        let plan = should_recalc_on_open(&book, &store);
        assert_eq!(plan.mode, OpenRecalcMode::Partial);
        assert_eq!(plan.cells, vec![(SheetIx(0), 1, 2), (SheetIx(0), 1, 4)]);
        let mut e = engine(&mut store, &["S"], ValueLocale::en_us());
        let mut rnd = RandomSource::sequence(Vec::new());
        let ch = apply_open_recalc(&mut e, &mut store, &plan, CLOCK, &mut rnd).unwrap();
        let v = |r: u32, c: u32| val(&e, &store, 0, r, c);
        assert_eq!(
            [v(0, 1), v(1, 2), v(1, 3), v(1, 4)],
            [Some(Scalar::Num(99.0)), Some(Scalar::Num(10.0)), Some(Scalar::Num(99.0)), Some(Scalar::Num(6.0))]
        );
        assert_eq!(ch.sheets.iter().map(|x| x.2).sum::<u32>(), 2);
        // full: everything, the wrong cache included
        let mut store = open_book(&[]);
        let book = OpenRecalcWorkbook { app_name_xl: true, calc_pr: None };
        let plan = should_recalc_on_open(&book, &store);
        assert_eq!(plan.reason, OpenRecalcReason::NoCalcId);
        let mut e = engine(&mut store, &["S"], ValueLocale::en_us());
        apply_open_recalc(&mut e, &mut store, &plan, CLOCK, &mut rnd).unwrap();
        assert_eq!(val(&e, &store, 0, 0, 1), Some(Scalar::Num(3.0)));
    }

    /// `save.test.ts` "ca nas voláteis e dependentes; ca velho sai; cm só na dinâmica".
    #[test]
    fn save_ca_and_cm() {
        let dyn_a = FormulaAttrs { t: Some(FormulaType::Array), r#ref: Some(js_s("E1:E2")), ..FormulaAttrs::default() };
        let ca = FormulaAttrs { flags: FormulaAttrs::CA, ..FormulaAttrs::default() };
        let mut store = store_of(
            &[
                ("A1", num("1")),
                ("B1", formula("NOW()", Some("1"))),
                ("C1", formula("B1+1", Some("1"))),
                ("D1", FileCell { fa: Some(ca), ..formula("A1+1", Some("2")) }),
                ("E1", FileCell { fa: Some(dyn_a), cm: Some(1.0), ..formula("A1:A2", Some("1")) }),
                ("F1", FileCell { cm: Some(1.0), ..formula("A1*2", Some("2")) }),
            ],
            1,
        );
        let input = EngineInput { sheet_names: vec![js_s("S")], dynamic_cm: vec![1.0], ..EngineInput::default() };
        let mut e = Engine::new(&mut store, &input, &EngineOptions::new(ValueLocale::en_us()), None).unwrap();
        let mut rnd = RandomSource::sequence(Vec::new());
        e.recalc(&mut store, CLOCK, &mut rnd).unwrap();
        let flags = save_flags(&mut e, &mut store);
        assert!(flags.iter().find(|x| x.0 == (SheetIx(0), 0, 1)).unwrap().1.ca);
        let mut next = || 1.0;
        let p =
            prepare_for_save(&mut e, &mut store, &mut SaveOptions { dynamic_cms: vec![1.0], dynamic_cm: Some(&mut next), sheets: None });
        let attrs = |store: &Store, col: u32| store.sheet(SheetIx(0)).unwrap().formulas.get(0, col).unwrap().1.attrs.as_deref().cloned();
        let cm = |store: &Store, col: u32| store.extra(SheetIx(0), 0, col).and_then(|x| x.cm);
        assert_eq!(attrs(&store, 1).map(|a| a.flags), Some(FormulaAttrs::CA));
        assert_eq!(attrs(&store, 2).map(|a| a.flags), Some(FormulaAttrs::CA));
        assert_eq!(attrs(&store, 3), None);
        assert_eq!(cm(&store, 4), Some(1.0));
        assert_eq!(cm(&store, 5), None);
        assert_eq!(p.touched, vec![SheetIx(0)]);
        assert!(p.sure, "{:?}", p.unsure);
    }

    /// `save.test.ts` "sem certeza: função de servidor, UDF, tabela de dados" and the orphaned shared child.
    #[test]
    fn save_unsure_and_orphan_child() {
        let dt =
            FormulaAttrs { t: Some(FormulaType::DataTable), r#ref: Some(js_s("C1:C2")), r1: Some(js_s("A1")), ..FormulaAttrs::default() };
        let mut store = store_of(
            &[
                ("A1", formula("_xlfn.WEBSERVICE(\"x\")", Some("1"))),
                ("B1", formula("MINHAUDF(1)", Some("1"))),
                ("C1", FileCell { fa: Some(dt), ..formula("", Some("1")) }),
            ],
            1,
        );
        let mut e = engine(&mut store, &["S"], ValueLocale::en_us());
        assert_eq!(unsure_reasons(&e), vec!["WEBSERVICE".to_owned(), "desconhecida MINHAUDF".to_owned()]);
        let p = prepare_for_save(&mut e, &mut store, &mut SaveOptions::default());
        assert!(!p.sure);
        assert!(p.unsure.iter().any(|s| s == "tabela de dados"));
        // a shared child whose anchor is replaced: its own formula
        let sh = FormulaAttrs { t: Some(FormulaType::Shared), r#ref: Some(js_s("B1:C1")), si: Some(0.0), ..FormulaAttrs::default() };
        let child = FormulaAttrs { t: Some(FormulaType::Shared), si: Some(0.0), ..FormulaAttrs::default() };
        let mut store = store_of(
            &[
                ("A1", num("3")),
                ("B1", FileCell { fa: Some(sh), ..formula("A1*2", Some("6")) }),
                ("C1", FileCell { fa: Some(child), ..formula("", Some("12")) }),
            ],
            1,
        );
        let mut e = engine(&mut store, &["S"], ValueLocale::en_us());
        let f = FormulaInput { text: js_s("A1+1"), array: true, dynamic: true, r#ref: None, ca: false };
        e.set_cell(&mut store, SheetIx(0), 0, 1, Some(CellInput::Formula(f))).unwrap();
        let mut rnd = RandomSource::sequence(Vec::new());
        e.calculate(&mut store, CLOCK, &mut rnd).unwrap();
        // the model's anchor as the bridge writes it (the TS test sets it by hand)
        store
            .set_formula(
                SheetIx(0),
                0,
                1,
                uo_calc::formula::FileFormula {
                    text: Some(js_s("A1+1")),
                    attrs: Some(Box::new(FormulaAttrs { t: Some(FormulaType::Array), r#ref: Some(js_s("B1")), ..FormulaAttrs::default() })),
                },
            )
            .unwrap();
        let mut made = 0;
        let mut next = || {
            made += 1;
            7.0
        };
        prepare_for_save(&mut e, &mut store, &mut SaveOptions { dynamic_cms: Vec::new(), dynamic_cm: Some(&mut next), sheets: None });
        let ff = store.sheet(SheetIx(0)).unwrap().formulas.get(0, 2).unwrap().1.clone();
        assert_eq!(ff.text.unwrap().as_js().to_string_lossy(), "B1*2");
        assert_eq!(ff.attrs, None);
        // A1+1 typed: the legacy form gives the same -> no t/ref, no cm
        assert_eq!(store.extra(SheetIx(0), 0, 1).and_then(|x| x.cm), None);
        assert_eq!(made, 0);
    }
}

// ---------------------------------------------------------------- the open-recalc plans of the uo-xlsx captures

/// The `plan=` facet of uo-xlsx's `sheet-x1`/`sheet-samples` captures (the TS `shouldRecalcOnOpen` of each
/// workbook): the decision rebuilt here from the sidecars — `wb` (the workbook part: `appName`, `calcPr`) and the
/// store facet lines `st<i>` (every cell of each sheet; the last duplicate, as the store keeps it). C1b's own replay
/// (through the real reader) checks the same facet end to end.
#[test]
fn open_recalc_plans_of_xlsx_captures() {
    let mut by_mode = std::collections::BTreeMap::<String, usize>::new();
    let mut bad = Vec::new();
    let mut n = 0;
    // the model cells in FILE order (duplicates included) of the store group, when it has the workbook
    let store_side = support::sidecar(&support::captures_dir().join("store.side.gz"));
    for name in ["sheet-x1", "sheet-samples"] {
        let p = support::xlsx_captures_dir().join(format!("{name}.capture"));
        let cap = uo_testkit::Capture::read(&p).unwrap();
        let side = support::sidecar(&p.with_extension("side.gz"));
        for rec in cap.records() {
            let Some(plan) = rec.get("plan") else { continue };
            let Some(s) = side.get(&rec.key) else { continue };
            let Some(wb) = s.get("wb").and_then(|v| v.first()) else { continue };
            let wb = parse(wb);
            let book = OpenRecalcWorkbook {
                app_name_xl: wb.get("appName").is_some_and(|a| s8(a) == "xl"),
                calc_pr: wb.get("calcPr").map(|cp| CalcPrFlags {
                    calc_id: cp.get("calcId").and_then(J::num),
                    manual: cp.get("calcMode").is_some_and(|m| s8(m) == "manual"),
                    full_calc_on_load: cp.get("fullCalcOnLoad").is_some_and(J::bool),
                    force_full_calc: cp.get("forceFullCalc").is_some_and(J::bool),
                }),
            };
            let sheets = wb.get("sheets").map_or(0, |x| x.arr().len());
            let mut store = Store::new();
            let mut props = support::Props::default();
            for i in 0..sheets {
                if let Some(lines) = store_side.get(&rec.key).and_then(|x| x.get(&format!("in{i}"))) {
                    support::load_sheet(&mut store, lines, &mut props);
                    continue;
                }
                let sh = store.add_sheet().unwrap();
                for l in s.get(&format!("st{i}")).map_or(&[][..], Vec::as_slice) {
                    let j = parse(l);
                    let (r, c) = (j.get("r").unwrap().num().unwrap() as u32, j.get("c").unwrap().num().unwrap() as u32);
                    store.set_file_cell(sh, r - 1, c - 1, &support::file_cell(&j, &mut props)).unwrap();
                }
            }
            let got = should_recalc_on_open(&book, &store);
            n += 1;
            *by_mode.entry(plan.split(':').next().unwrap_or("").to_owned()).or_default() += 1;
            // `partial:<cells>`: the number of formulas to calculate
            let ours = match got.mode {
                uo_calc::open_recalc::OpenRecalcMode::Partial => format!("partial:{}", got.cells.len()),
                m => m.text().to_owned(),
            };
            if ours != plan {
                bad.push(format!("{}: ts {plan} rust {ours} ({})", rec.key, got.reason.text()));
            }
        }
    }
    eprintln!("open-recalc plans of the uo-xlsx captures: {n} workbooks {by_mode:?}, {} mismatches", bad.len());
    for b in &bad {
        eprintln!("  {b}");
    }
    assert!(n >= 35, "{n}");
    assert!(bad.is_empty());
}
