//! The formula language of the entry (C3a-8 closed): `fromLocal`/`toLocal` of the calc's i18n (A5's
//! [`Translator`]) with the locale's syntax (`{ lang, decimal, list }`, `formula-locale.ts:63`), the workbook's
//! names and the table of the cell; the reserved-function and argument-count checks of the catalogue.

use std::cell::RefCell;

use uo_calc::catalogue::{FunctionKind, accepts_arg_count, catalog_entry, function_kind};
use uo_calc::i18n::{LangSpec, NameSet, SyntaxOptions, Translator};
use uo_calc::{JsStr, JsString};

use crate::grid::{Arity, EntryContext, FormulaLang, FormulaLocale, FormulaSyntax};
use crate::read::model::Workbook;

/// `names()`: the defined names without the `_xlnm.` prefix (`/^_xlnm\./i`).
pub(crate) fn names_of(wb: &Workbook) -> Vec<String> {
    wb.defined_names
        .iter()
        .map(|d| {
            let n = &d.name;
            if n.len() >= 6 && n.is_char_boundary(6) && n[..6].eq_ignore_ascii_case("_xlnm.") { n[6..].to_owned() } else { n.clone() }
        })
        .collect()
}

/// The locale's translator as the entry's [`FormulaSyntax`].
pub(crate) struct LocaleSyntax<'a> {
    locale: &'a FormulaLocale,
    tr: &'a RefCell<Translator>,
    decimal: JsString,
    list: JsString,
}

impl<'a> LocaleSyntax<'a> {
    pub(crate) fn new(locale: &'a FormulaLocale, tr: &'a RefCell<Translator>) -> Self {
        LocaleSyntax { locale, tr, decimal: JsString::from_str_js(&locale.decimal), list: JsString::from_str_js(&locale.list) }
    }

    fn lang_id(&self) -> &'static str {
        if self.locale.lang == FormulaLang::PtBr { "pt-BR" } else { "en-US" }
    }

    /// `toLocal(canonical, { ...syntax, names, inTable })`.
    pub(crate) fn to_local(&self, canonical: &[u16], names: &[String], in_table: Option<JsStr<'_>>) -> Vec<u16> {
        let set = name_set_utf16(names);
        let opts = SyntaxOptions {
            lang: LangSpec::Id(self.lang_id()),
            decimal: Some(self.decimal.as_js()),
            list: Some(self.list.as_js()),
            names: Some(&set),
            in_table,
            ..SyntaxOptions::default()
        };
        let mut out = Vec::new();
        self.tr.borrow_mut().to_local_into(JsStr::Utf16(canonical), &opts, &mut out);
        out
    }
}

fn name_set_utf16(names: &[String]) -> NameSet {
    let units: Vec<Vec<u16>> = names.iter().map(|n| n.encode_utf16().collect()).collect();
    NameSet::new(units.iter().map(|u| JsStr::Utf16(u)))
}

impl FormulaSyntax for LocaleSyntax<'_> {
    fn to_canonical(&self, local: &[u16], ctx: &EntryContext<'_>) -> Option<Vec<u16>> {
        let names: Vec<String> = ctx.names.iter().map(|s| (*s).to_owned()).collect();
        let set = name_set_utf16(&names);
        let table = ctx.in_table.map(|t| t.encode_utf16().collect::<Vec<u16>>());
        let opts = SyntaxOptions {
            lang: LangSpec::Id(self.lang_id()),
            decimal: Some(self.decimal.as_js()),
            list: Some(self.list.as_js()),
            file_prefixes: true,
            names: Some(&set),
            in_table: table.as_deref().map(JsStr::Utf16),
            ..SyntaxOptions::default()
        };
        let mut out = Vec::new();
        self.tr.borrow_mut().from_local_into(JsStr::Utf16(local), &opts, &mut out);
        Some(out)
    }

    fn is_reserved(&self, canonical_name: &[u16]) -> bool {
        function_kind(JsStr::Utf16(canonical_name)) == FunctionKind::Reserved
    }

    fn arity(&self, canonical_name: &[u16], n_args: usize) -> Arity {
        let Some(e) = catalog_entry(JsStr::Utf16(canonical_name)) else { return Arity::Ok };
        let n = n_args as f64;
        if accepts_arg_count(e, n) {
            Arity::Ok
        } else if n < f64::from(e.min_args) {
            Arity::Few
        } else {
            Arity::Many
        }
    }
}
