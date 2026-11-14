//! The style a field names (WordCraft study, area "fields", item B21; record `core/docs/modules/uo-docx-layout.md`
//! "WordCraft study — fields"): how Word resolves `STYLEREF "name"`, measured in Word 365 pt-BR (`oracle_wc/styleref`):
//! case-insensitively, the built-in style's name in the UI language ("Título 1", "título 1"), a custom style's
//! `w:name` ("Meu Estilo") and any of its `w:aliases` ("Cap"); NOT the English built-in name in a pt-BR UI ("heading 1",
//! "Heading 1"), the styleId ("Ttulo1") or an unaccented spelling ("Titulo 1"). The en/es UI rules follow the same
//! mechanism (not measured: only pt-BR Office is installed).

use crate::model::formatting::{DocFormatting, StyleDef};

/// What a name resolves to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StyleTarget {
    /// a paragraph style (its id)
    Para(Box<str>),
    /// a character style (its id)
    Char(Box<str>),
}

/// The UI language of the names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ui {
    Pt,
    En,
    Es,
}

fn ui_of(lang: &str) -> Ui {
    match super::numwords::WordsLang::of(lang) {
        super::numwords::WordsLang::Pt => Ui::Pt,
        super::numwords::WordsLang::Es => Ui::Es,
        super::numwords::WordsLang::En => Ui::En,
    }
}

fn lower(s: &str) -> String {
    uo_text::case::to_lowercase_str(s)
}

/// The name a style shows in the UI language: a built-in's localized name (pt from Word's own table of built-in
/// styles, es from the Spanish table, en the canonical name), else its `w:name`.
fn ui_name(def: &StyleDef, ui: Ui) -> Option<String> {
    let name = def.name.as_deref()?;
    let k = lower(name);
    let builtin = crate::data::word_styles::WORD_BUILTIN_STYLES.iter().find(|(n, ..)| lower(n) == k);
    Some(match (builtin, ui) {
        (Some((_, pt, ..)), Ui::Pt) => (*pt).to_owned(),
        (Some(_), Ui::Es) => crate::read::style_names_es::builtin_style_name_es(&k).map_or_else(|| name.to_owned(), str::to_owned),
        _ => name.to_owned(),
    })
}

/// The paragraph or character style `name` designates in a UI language (see the module comment); `None` when no style
/// of the document answers to it.
#[must_use]
pub fn style_by_ui_name(fmt: &DocFormatting, name: &str, ui_lang: &str) -> Option<StyleTarget> {
    let want = lower(uo_jsnum::text::trim(name));
    if want.is_empty() {
        return None;
    }
    let ui = ui_of(ui_lang);
    for (id, def) in fmt.styles.styles.iter() {
        let make = || match &*def.r#type {
            "paragraph" => Some(StyleTarget::Para(id.into())),
            "character" => Some(StyleTarget::Char(id.into())),
            _ => None,
        };
        let by_name = ui_name(def, ui).is_some_and(|n| lower(&n) == want);
        let by_alias = def.aliases.0.as_deref().is_some_and(|a| a.split(',').any(|x| lower(uo_jsnum::text::trim(x)) == want));
        if (by_name || by_alias)
            && let Some(t) = make()
        {
            return Some(t);
        }
    }
    None
}

/// Word's STYLEREF error for a name no style of the document answers to (pt measured; en/es Word's UI strings,
/// unverified).
#[must_use]
pub fn style_ref_unknown_error(ui_lang: &str, name: &str) -> String {
    match ui_of(ui_lang) {
        Ui::Pt => format!("Erro! Use a guia Página Inicial para aplicar {name} ao texto que deverá aparecer aqui."),
        Ui::Es => format!("¡Error! Utilice la pestaña Inicio para aplicar {name} al texto que desea que aparezca aquí."),
        Ui::En => format!("Error! Use the Home tab to apply {name} to the text that you want to appear here."),
    }
}

/// Word's STYLEREF error when no text has the style (pt measured; es Word's UI string, unverified).
#[must_use]
pub fn style_ref_no_text_error(ui_lang: &str) -> &'static str {
    match ui_of(ui_lang) {
        Ui::Pt => "Erro! Nenhum texto com o estilo especificado foi encontrado no documento.",
        Ui::Es => "¡Error! No hay texto con el estilo especificado en el documento.",
        Ui::En => "Error! No text of specified style in document.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::formatting::{StyleAliases, StyleDef};

    fn fmt() -> DocFormatting {
        let mut f = DocFormatting::default();
        let mut add = |id: &str, ty: &str, name: &str, aliases: Option<&str>| {
            let d = StyleDef {
                style_id: id.into(),
                r#type: ty.into(),
                name: Some(name.into()),
                aliases: StyleAliases(aliases.map(Into::into)),
                ..StyleDef::default()
            };
            f.styles.styles.insert(id, d);
        };
        add("Ttulo1", "paragraph", "heading 1", None);
        add("MeuEstilo", "paragraph", "Meu Estilo", Some("Cap,Outro"));
        add("Destaque", "character", "Destaque", None);
        f
    }

    #[test]
    fn names_as_word_resolves_them() {
        let f = fmt();
        let para = |id: &str| Some(StyleTarget::Para(id.into()));
        assert_eq!(style_by_ui_name(&f, "Título 1", "pt-BR"), para("Ttulo1"));
        assert_eq!(style_by_ui_name(&f, "título 1", "pt-BR"), para("Ttulo1"));
        assert_eq!(style_by_ui_name(&f, "heading 1", "pt-BR"), None);
        assert_eq!(style_by_ui_name(&f, "Heading 1", "pt-BR"), None);
        assert_eq!(style_by_ui_name(&f, "Ttulo1", "pt-BR"), None);
        assert_eq!(style_by_ui_name(&f, "Titulo 1", "pt-BR"), None);
        assert_eq!(style_by_ui_name(&f, "meu estilo", "pt-BR"), para("MeuEstilo"));
        assert_eq!(style_by_ui_name(&f, "Cap", "pt-BR"), para("MeuEstilo"));
        assert_eq!(style_by_ui_name(&f, "Destaque", "pt-BR"), Some(StyleTarget::Char("Destaque".into())));
        assert_eq!(style_by_ui_name(&f, "Heading 1", "en-US"), para("Ttulo1"));
        assert_eq!(style_by_ui_name(&f, "Título 1", "es-ES"), para("Ttulo1"));
        assert_eq!(
            style_ref_unknown_error("pt-BR", "Inexistente"),
            "Erro! Use a guia Página Inicial para aplicar Inexistente ao texto que deverá aparecer aqui."
        );
    }
}
