//! The lexer and parser of the `.uos` IDL. The grammar is documented in the record
//! `core/docs/modules/uo-render.md` ("Milestone 1 — frontier C", "IDL grammar").

use std::fmt;

/// A parse or validation error with the 1-based line it refers to (0 when it has none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub line: usize,
    pub msg: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 { write!(f, "{}", self.msg) } else { write!(f, "line {}: {}", self.line, self.msg) }
    }
}

impl std::error::Error for Error {}

pub(crate) fn err<T>(line: usize, msg: impl Into<String>) -> Result<T, Error> {
    Err(Error { line, msg: msg.into() })
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Punct(char),
    Doc(String),
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    line: usize,
}

fn lex(src: &str) -> Result<Vec<Token>, Error> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            let doc = chars.get(i + 2) == Some(&'/');
            let start = i + if doc { 3 } else { 2 };
            let mut j = start;
            while j < chars.len() && chars[j] != '\n' {
                j += 1;
            }
            if doc {
                let text: String = chars[start..j].iter().collect();
                out.push(Token { tok: Tok::Doc(text.trim().to_string()), line });
            }
            i = j;
        } else if c.is_ascii_alphabetic() || c == '_' {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            out.push(Token { tok: Tok::Ident(chars[i..j].iter().collect()), line });
            i = j;
        } else if c == '"' {
            let mut j = i + 1;
            let mut s = String::new();
            loop {
                match chars.get(j) {
                    None | Some('\n') => return err(line, "unterminated string"),
                    Some('"') => break,
                    Some(&ch) if ch.is_ascii() && !ch.is_control() => s.push(ch),
                    Some(_) => return err(line, "strings are printable ASCII"),
                }
                j += 1;
            }
            out.push(Token { tok: Tok::Str(s), line });
            i = j + 1;
        } else if "{}()<>,:;=?".contains(c) {
            out.push(Token { tok: Tok::Punct(c), line });
            i += 1;
        } else {
            return err(line, format!("unexpected character {c:?}"));
        }
    }
    out.push(Token { tok: Tok::Eof, line });
    Ok(out)
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstTy {
    Prim(String),
    Named(String),
    Ref(String),
    List(Box<AstTy>),
    Columns(String),
}

#[derive(Debug, Clone)]
pub struct AstField {
    pub name: String,
    pub ty: AstTy,
    pub optional: bool,
    pub ts: Option<String>,
    pub doc: Vec<String>,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct AstVariant {
    pub name: String,
    pub ts: Option<String>,
    pub payload: Option<AstTy>,
    pub doc: Vec<String>,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub enum AstItem {
    Table { name: String, entry: String, line: usize },
    Struct { name: String, fields: Vec<AstField>, message: bool, doc: Vec<String>, line: usize },
    Enum { name: String, tag: Option<String>, variants: Vec<AstVariant>, doc: Vec<String>, line: usize },
}

#[derive(Debug, Clone)]
pub struct Ast {
    pub name: String,
    pub magic: String,
    pub items: Vec<AstItem>,
}

pub const PRIMITIVES: [&str; 11] = ["bool", "u8", "u16", "u32", "i32", "f64", "utf16", "sym", "bytes", "app_section", "bytes_ref"];

struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.toks[self.pos.min(self.toks.len() - 1)]
    }

    fn line(&self) -> usize {
        self.peek().line
    }

    fn next(&mut self) -> Token {
        let t = self.peek().clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn describe(t: &Tok) -> String {
        match t {
            Tok::Ident(s) => format!("`{s}`"),
            Tok::Str(s) => format!("\"{s}\""),
            Tok::Punct(c) => format!("`{c}`"),
            Tok::Doc(_) => "a doc comment".into(),
            Tok::Eof => "the end of the file".into(),
        }
    }

    fn punct(&mut self, c: char) -> Result<(), Error> {
        let t = self.next();
        if t.tok == Tok::Punct(c) { Ok(()) } else { err(t.line, format!("expected `{c}`, found {}", Self::describe(&t.tok))) }
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek().tok == Tok::Punct(c) {
            self.next();
            true
        } else {
            false
        }
    }

    fn ident(&mut self, what: &str) -> Result<String, Error> {
        let t = self.next();
        match t.tok {
            Tok::Ident(s) => Ok(s),
            other => err(t.line, format!("expected {what}, found {}", Self::describe(&other))),
        }
    }

    fn string(&mut self, what: &str) -> Result<String, Error> {
        let t = self.next();
        match t.tok {
            Tok::Str(s) => Ok(s),
            other => err(t.line, format!("expected {what}, found {}", Self::describe(&other))),
        }
    }

    fn keyword(&mut self, kw: &str) -> Result<(), Error> {
        let t = self.next();
        match &t.tok {
            Tok::Ident(s) if s == kw => Ok(()),
            other => err(t.line, format!("expected `{kw}`, found {}", Self::describe(other))),
        }
    }

    fn peek_ident(&self, kw: &str) -> bool {
        matches!(&self.peek().tok, Tok::Ident(s) if s == kw)
    }

    fn docs(&mut self) -> Vec<String> {
        let mut v = Vec::new();
        while let Tok::Doc(d) = &self.peek().tok {
            v.push(d.clone());
            self.next();
        }
        v
    }

    fn base_ty(&mut self) -> Result<AstTy, Error> {
        let line = self.line();
        let name = self.ident("a type")?;
        match name.as_str() {
            "ref" => {
                self.punct('<')?;
                let t = self.ident("a table name")?;
                self.punct('>')?;
                Ok(AstTy::Ref(t))
            }
            "list" => {
                self.punct('<')?;
                let inner = self.base_ty()?;
                self.punct('>')?;
                Ok(AstTy::List(Box::new(inner)))
            }
            "columns" => {
                self.punct('<')?;
                let t = self.ident("a struct name")?;
                self.punct('>')?;
                Ok(AstTy::Columns(t))
            }
            n if PRIMITIVES.contains(&n) => Ok(AstTy::Prim(name)),
            _ => {
                if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    Ok(AstTy::Named(name))
                } else {
                    err(line, format!("unknown type `{name}`"))
                }
            }
        }
    }

    fn fields(&mut self) -> Result<Vec<AstField>, Error> {
        self.punct('{')?;
        let mut fields = Vec::new();
        loop {
            let doc = self.docs();
            if self.eat('}') {
                break;
            }
            let line = self.line();
            let name = self.ident("a field name")?;
            self.punct(':')?;
            let ty = self.base_ty()?;
            let optional = self.eat('?');
            let ts = if self.peek_ident("ts") {
                self.next();
                Some(self.string("a TypeScript name")?)
            } else {
                None
            };
            fields.push(AstField { name, ty, optional, ts, doc, line });
            if !self.eat(',') {
                self.docs();
                self.punct('}')?;
                break;
            }
        }
        Ok(fields)
    }

    fn variants(&mut self) -> Result<Vec<AstVariant>, Error> {
        self.punct('{')?;
        let mut out = Vec::new();
        loop {
            let doc = self.docs();
            if self.eat('}') {
                break;
            }
            let line = self.line();
            let name = self.ident("a variant name")?;
            let ts = if self.eat('=') { Some(self.string("a TypeScript name")?) } else { None };
            let payload = if self.eat('(') {
                let t = self.base_ty()?;
                self.punct(')')?;
                Some(t)
            } else {
                None
            };
            out.push(AstVariant { name, ts, payload, doc, line });
            if !self.eat(',') {
                self.docs();
                self.punct('}')?;
                break;
            }
        }
        Ok(out)
    }

    fn file(&mut self) -> Result<Ast, Error> {
        self.docs();
        self.keyword("schema")?;
        let name = self.ident("the schema name")?;
        self.keyword("magic")?;
        let magic = self.string("the 4-character magic")?;
        self.punct(';')?;
        let mut items = Vec::new();
        loop {
            let doc = self.docs();
            let line = self.line();
            let t = self.next();
            match t.tok {
                Tok::Eof => break,
                Tok::Ident(kw) => match kw.as_str() {
                    "table" => {
                        let name = self.ident("a table name")?;
                        self.punct(':')?;
                        let entry = self.ident("the entry struct")?;
                        self.punct(';')?;
                        items.push(AstItem::Table { name, entry, line });
                    }
                    "struct" | "message" => {
                        let name = self.ident("a struct name")?;
                        let fields = self.fields()?;
                        items.push(AstItem::Struct { name, fields, message: kw == "message", doc, line });
                    }
                    "enum" => {
                        let name = self.ident("an enum name")?;
                        let tag = if self.peek_ident("tag") {
                            self.next();
                            Some(self.string("the TypeScript tag property")?)
                        } else {
                            None
                        };
                        let variants = self.variants()?;
                        items.push(AstItem::Enum { name, tag, variants, doc, line });
                    }
                    other => return err(t.line, format!("expected `table`, `struct`, `message` or `enum`, found `{other}`")),
                },
                other => return err(t.line, format!("expected a declaration, found {}", Self::describe(&other))),
            }
        }
        Ok(Ast { name, magic, items })
    }
}

pub fn parse(src: &str) -> Result<Ast, Error> {
    let toks = lex(src)?;
    Parser { toks, pos: 0 }.file()
}
