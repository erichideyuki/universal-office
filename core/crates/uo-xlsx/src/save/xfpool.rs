//! The XfPool (`sheet-calc.ts:511-853`): the cell formats the edits derive (number format, text prefix, font,
//! fill, borders, alignment). It reuses what was created, appends to the model's `Styles` as it goes and, at
//! save, patches `styles.xml` by regex surgery: the new fonts/fills/borders/`<xf>`s (COPIED from the source xf,
//! so protection, `extLst`… stay) and the new `numFmt`s, nothing else touched. The patch is the TS's text
//! surgery on the same ECMAScript engine (`rx.rs`).

use super::SaveError;
use super::rx::Rx;
use super::text::{esc_attr, put_num};
use crate::read::model::{Border, BorderEdge, CellXf, Fill, Font, Styles};

/// Ids of the built-in number formats that do not depend on the language (14-22 do; 41-44 have no fixed code).
const FIXED_BUILTINS: &[(u32, &str)] = &[
    (0, "General"),
    (1, "0"),
    (2, "0.00"),
    (3, "#,##0"),
    (4, "#,##0.00"),
    (9, "0%"),
    (10, "0.00%"),
    (11, "0.00E+00"),
    (12, "# ?/?"),
    (13, "# ??/??"),
    (37, "#,##0_);(#,##0)"),
    (38, "#,##0_);[Red](#,##0)"),
    (39, "#,##0.00_);(#,##0.00)"),
    (40, "#,##0.00_);[Red](#,##0.00)"),
    (45, "mm:ss"),
    (46, "[h]:mm:ss"),
    (47, "mm:ss.0"),
    (48, "##0.0E+0"),
    (49, "@"),
];

/// A cell format of the pool: the model's xf plus the text prefix (`'`), which the reader does not read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PoolXf {
    /// The xf as the model holds it.
    pub xf: CellXf,
    /// `quotePrefix`.
    pub quote_prefix: bool,
}

/// `NumFmtHint` of the entry: a format id and, for a new one, its code.
#[derive(Clone, Debug, PartialEq)]
pub struct NumFmtChange {
    /// `numFmtId`.
    pub id: f64,
    /// The code, when the id is not a built-in.
    pub code: Option<String>,
}

/// `FontChange`; `None` keeps. `color: Some(None)` = automatic.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontChange {
    /// Bold.
    pub bold: Option<bool>,
    /// Italic.
    pub italic: Option<bool>,
    /// Underline.
    pub underline: Option<bool>,
    /// Strike.
    pub strike: Option<bool>,
    /// Size in points.
    pub size: Option<f64>,
    /// Name.
    pub name: Option<String>,
    /// Color `#RRGGBB` (`Some(None)` removes it).
    pub color: Option<Option<String>>,
}

/// `AlignChange`; `Some(None)` = back to the default (General / Bottom).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlignChange {
    /// Horizontal.
    pub horizontal: Option<Option<String>>,
    /// Vertical.
    pub vertical: Option<Option<String>>,
    /// Wrap text.
    pub wrap_text: Option<bool>,
    /// Indent.
    pub indent: Option<f64>,
}

/// `StyleChange`: what the bars apply to a cell. `fill: Some(None)` = no fill; `border: Some(None)` = no borders
/// (a given border REPLACES the cell's).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleChange {
    /// Number format.
    pub num_fmt: Option<NumFmtChange>,
    /// Text prefix.
    pub quote_prefix: Option<bool>,
    /// Font.
    pub font: Option<FontChange>,
    /// Fill color.
    pub fill: Option<Option<String>>,
    /// Border.
    pub border: Option<Option<Border>>,
    /// Alignment.
    pub align: Option<AlignChange>,
}

#[derive(Clone, Debug)]
struct AddedXf {
    src: usize,
    rec: PoolXf,
}

#[derive(Clone, Debug)]
struct AddedFont {
    /// The font OF THE FILE whose XML is the template (family, charset, scheme…).
    tpl: usize,
    font: Font,
}

/// The pool. The model's [`Styles`] is passed to every call (the TS holds a reference to it).
#[derive(Clone, Debug, Default)]
pub struct XfPool {
    base: usize,
    font_base: usize,
    fill_base: usize,
    border_base: usize,
    added: Vec<AddedXf>,
    fonts: Vec<AddedFont>,
    fills: Vec<Fill>,
    borders: Vec<Border>,
    fmts: Vec<(f64, String)>,
    /// defaults put in place of a block that was missing in the file (font, the 2 fills, the empty border)
    defaults: (usize, usize, usize),
}

fn same_color(a: Option<&str>, b: Option<&str>) -> bool {
    uo_text::case::to_uppercase_str(a.unwrap_or("")) == uo_text::case::to_uppercase_str(b.unwrap_or(""))
}

fn same_xf(a: &PoolXf, b: &PoolXf) -> bool {
    a.xf.num_fmt_id == b.xf.num_fmt_id
        && a.xf.font_id == b.xf.font_id
        && a.xf.fill_id == b.xf.fill_id
        && a.xf.border_id == b.xf.border_id
        && a.quote_prefix == b.quote_prefix
        && a.xf.horizontal == b.xf.horizontal
        && a.xf.vertical == b.xf.vertical
        && a.xf.wrap_text == b.xf.wrap_text
        && a.xf.indent == b.xf.indent
}

fn same_font(a: &Font, b: &Font) -> bool {
    a.bold == b.bold
        && a.italic == b.italic
        && a.underline == b.underline
        && a.underline_style == b.underline_style
        && a.strike == b.strike
        && a.size == b.size
        && same_color(a.color.as_deref(), b.color.as_deref())
        && a.name == b.name
        && a.alt_name == b.alt_name
        && a.vert_align == b.vert_align
}

fn same_edge(a: &Option<BorderEdge>, b: &Option<BorderEdge>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x.style == y.style && same_color(Some(&x.color), Some(&y.color)),
        _ => false,
    }
}

fn same_border(a: &Border, b: &Border) -> bool {
    same_edge(&a.left, &b.left)
        && same_edge(&a.right, &b.right)
        && same_edge(&a.top, &b.top)
        && same_edge(&a.bottom, &b.bottom)
        && same_edge(&a.diagonal, &b.diagonal)
        && a.diagonal_up == b.diagonal_up
        && a.diagonal_down == b.diagonal_down
}

fn has_edges(b: &Border) -> bool {
    b.left.is_some() || b.right.is_some() || b.top.is_some() || b.bottom.is_some() || b.diagonal.is_some()
}

fn num_text(n: f64) -> String {
    let mut b = Vec::new();
    put_num(&mut b, n);
    String::from_utf8_lossy(&b).into_owned()
}

/// `argb(c)`: `#RRGGBB` to the OOXML ARGB.
fn argb(c: &str) -> String {
    let s = uo_text::case::to_uppercase_str(&c.replacen('#', "", 1));
    let pad: String = format!("{s:0>6}");
    let last: Vec<char> = pad.chars().collect();
    let tail: String = last[last.len().saturating_sub(6)..].iter().collect();
    format!("FF{tail}")
}

impl XfPool {
    /// A pool over the model as it was read.
    #[must_use]
    pub fn new(styles: &Styles) -> Self {
        XfPool {
            base: styles.cell_xfs.len(),
            font_base: styles.fonts.len(),
            fill_base: styles.fills.len(),
            border_base: styles.borders.len(),
            ..XfPool::default()
        }
    }

    /// The number of formats of the file (the first derived xf has this index).
    #[must_use]
    pub fn base(&self) -> usize {
        self.base
    }

    /// Nothing was derived (the patch would not change `styles.xml`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.fmts.is_empty() && self.fonts.is_empty() && self.fills.is_empty() && self.borders.is_empty()
    }

    fn xf_at(&self, styles: &Styles, i: usize) -> Option<PoolXf> {
        if i >= self.base {
            self.added.get(i - self.base).map(|a| a.rec.clone())
        } else {
            styles.cell_xfs.get(i).map(|x| PoolXf { xf: x.clone(), quote_prefix: false })
        }
    }

    /// `quotePrefix(xf)`.
    #[must_use]
    pub fn quote_prefix(&self, xf: usize) -> bool {
        xf >= self.base && self.added.get(xf - self.base).is_some_and(|a| a.rec.quote_prefix)
    }

    /// `derive(from, fmt, quotePrefix)`: the xf equal to `from` with another format and/or text prefix.
    pub fn derive(&mut self, styles: &mut Styles, from: usize, fmt: Option<NumFmtChange>, quote_prefix: bool) -> usize {
        let change = StyleChange { quote_prefix: Some(quote_prefix), num_fmt: fmt, ..StyleChange::default() };
        self.apply(styles, from, &change)
    }

    /// `formatId(code)`: the id of the file's, a fixed built-in's, or a new one (>= 164).
    pub fn format_id(&mut self, styles: &mut Styles, code: &str) -> f64 {
        if let Some((id, _)) = styles.num_fmts.iter().find(|(_, c)| c == code) {
            return *id;
        }
        if let Some((id, _)) = FIXED_BUILTINS.iter().find(|(_, c)| *c == code) {
            return f64::from(*id);
        }
        let mut id = 163.0f64;
        for (k, _) in &styles.num_fmts {
            if *k > id {
                id = *k;
            }
        }
        id += 1.0;
        set_fmt(&mut styles.num_fmts, id, code);
        set_fmt(&mut self.fmts, id, code);
        id
    }

    /// `apply(from, change)`: the xf equal to `from` with the change (`from` itself when nothing changes).
    pub fn apply(&mut self, styles: &mut Styles, from: usize, change: &StyleChange) -> usize {
        let src = self.xf_at(styles, from).or_else(|| self.xf_at(styles, 0)).unwrap_or_default();
        let mut rec = src.clone();
        if let Some(nf) = &change.num_fmt {
            rec.xf.num_fmt_id = nf.id;
            rec.xf.apply_number_format = true;
            if let Some(code) = &nf.code
                && !styles.num_fmts.iter().any(|(k, _)| *k == nf.id)
            {
                set_fmt(&mut styles.num_fmts, nf.id, code);
                set_fmt(&mut self.fmts, nf.id, code);
            }
        }
        if let Some(q) = change.quote_prefix {
            rec.quote_prefix = q;
        }
        if let Some(f) = &change.font {
            rec.xf.font_id = self.font_for(styles, src.xf.font_id, f);
        }
        if let Some(fill) = &change.fill {
            rec.xf.fill_id = match fill {
                None => 0.0,
                Some(c) => self.fill_for(styles, c),
            };
        }
        if let Some(b) = &change.border {
            rec.xf.border_id = match b {
                Some(b) if has_edges(b) => self.border_for(styles, b),
                _ => 0.0,
            };
        }
        if let Some(a) = &change.align {
            if let Some(h) = &a.horizontal {
                rec.xf.horizontal = h.clone().filter(|h| h != "general");
            }
            if let Some(v) = &a.vertical {
                rec.xf.vertical = v.clone().filter(|v| v != "bottom");
            }
            if let Some(w) = a.wrap_text {
                rec.xf.wrap_text = w;
            }
            if let Some(i) = a.indent {
                rec.xf.indent = if i > 0.0 { Some(i) } else { None };
            }
        }
        if rec.xf.font_id != src.xf.font_id {
            rec.xf.apply_font = true;
        }
        if rec.xf.fill_id != src.xf.fill_id {
            rec.xf.apply_fill = true;
        }
        if rec.xf.border_id != src.xf.border_id {
            rec.xf.apply_border = true;
        }
        if change.align.is_some() {
            rec.xf.apply_alignment = true;
        }
        if same_xf(&rec, &src) {
            return from;
        }
        let origin = if from >= self.base { self.added.get(from - self.base).map_or(from, |a| a.src) } else { from };
        // already created: same origin and the same result
        if let Some(i) = self.added.iter().position(|a| a.src == origin && same_xf(&a.rec, &rec)) {
            return self.base + i;
        }
        // the origin itself serves (back to the file's style)
        if let Some(o) = styles.cell_xfs.get(origin).filter(|_| origin < self.base) {
            let o = PoolXf { xf: o.clone(), quote_prefix: false };
            if same_xf(&o, &rec) {
                return origin;
            }
        }
        styles.cell_xfs.push(rec.xf.clone());
        self.added.push(AddedXf { src: origin, rec });
        styles.cell_xfs.len() - 1
    }

    fn font_for(&mut self, styles: &mut Styles, font_id: f64, ch: &FontChange) -> f64 {
        if styles.fonts.is_empty() {
            styles.fonts.push(Font { name: Some("Calibri".into()), size: Some(11.0), ..Font::default() });
            self.defaults.0 = 1;
        }
        let idx =
            if font_id >= 0.0 && font_id.fract() == 0.0 && (font_id as usize) < styles.fonts.len() { Some(font_id as usize) } else { None };
        let cur = idx.map_or_else(|| styles.fonts[0].clone(), |i| styles.fonts[i].clone());
        let mut f = cur.clone();
        if let Some(v) = ch.bold {
            f.bold = v;
        }
        if let Some(v) = ch.italic {
            f.italic = v;
        }
        if let Some(v) = ch.strike {
            f.strike = v;
        }
        if let Some(u) = ch.underline {
            f.underline = u;
            if !u {
                f.underline_style = None;
            }
        }
        if let Some(s) = ch.size {
            f.size = Some(s);
        }
        if let Some(n) = &ch.name
            && Some(n) != cur.name.as_ref()
        {
            f.name = Some(n.clone());
            f.alt_name = None;
        }
        if let Some(c) = &ch.color {
            f.color = c.as_ref().map(|c| uo_text::case::to_uppercase_str(c));
        }
        if same_font(&f, &cur) {
            return idx.map_or(0.0, |i| i as f64);
        }
        if let Some(hit) = styles.fonts.iter().position(|x| same_font(x, &f)) {
            return hit as f64;
        }
        let tpl = match idx {
            Some(i) if i < self.font_base => i,
            Some(i) => self.fonts.get(i - self.font_base).map_or(0, |a| a.tpl),
            None => 0,
        };
        styles.fonts.push(f.clone());
        self.fonts.push(AddedFont { tpl, font: f });
        (styles.fonts.len() - 1) as f64
    }

    fn fill_for(&mut self, styles: &mut Styles, color: &str) -> f64 {
        if styles.fills.is_empty() {
            self.defaults.1 = 2;
        }
        while styles.fills.len() < 2 {
            let pattern = if styles.fills.is_empty() { "none" } else { "gray125" };
            styles.fills.push(Fill { pattern: Some(pattern.into()), ..Fill::default() });
        }
        let hit = styles
            .fills
            .iter()
            .position(|x| x.pattern.as_deref() == Some("solid") && x.gradient.is_none() && same_color(x.fg_color.as_deref(), Some(color)));
        if let Some(h) = hit {
            return h as f64;
        }
        let f = Fill { pattern: Some("solid".into()), fg_color: Some(uo_text::case::to_uppercase_str(color)), ..Fill::default() };
        styles.fills.push(f.clone());
        self.fills.push(f);
        (styles.fills.len() - 1) as f64
    }

    fn border_for(&mut self, styles: &mut Styles, b: &Border) -> f64 {
        if styles.borders.is_empty() {
            styles.borders.push(Border::default());
            self.defaults.2 = 1;
        }
        if let Some(h) = styles.borders.iter().position(|x| same_border(x, b)) {
            return h as f64;
        }
        let edge = |e: &Option<BorderEdge>| {
            e.as_ref().map(|e| BorderEdge { style: e.style.clone(), color: uo_text::case::to_uppercase_str(&e.color) })
        };
        let nb = Border {
            left: edge(&b.left),
            right: edge(&b.right),
            top: edge(&b.top),
            bottom: edge(&b.bottom),
            diagonal: edge(&b.diagonal),
            ..Border::default()
        };
        styles.borders.push(nb.clone());
        self.borders.push(nb);
        (styles.borders.len() - 1) as f64
    }

    /// `patch(xml)`: appends the new fonts/fills/borders/xfs and numFmts to `styles.xml` (nothing else touched).
    /// `styles` is the model the pool derived into (the template fonts and source xfs are read from it).
    pub fn patch(&self, styles: &Styles, xml: &str) -> Result<String, SaveError> {
        if self.is_empty() {
            return Ok(xml.to_owned());
        }
        let mut out = xml.to_owned();
        let p = Rx::new(r"<((?:\w+:)?)styleSheet\b")?.find(&out, 0).and_then(|m| m.group(1)).map(|r| out[r].to_owned()).unwrap_or_default();
        // the items OF THE FILE of each block (writing again does not duplicate the added ones)
        struct Block {
            items: Vec<String>,
            at: usize,
            len: usize,
        }
        let block = |out: &str, name: &str, item: &str, base: usize| -> Result<Option<Block>, SaveError> {
            let Some(m) = Rx::new(&format!(r"<{p}{name}\b[^>]*?(?:/>|>([\s\S]*?)</{p}{name}>)"))?.find(out, 0) else { return Ok(None) };
            let inner = m.group(1).map_or("", |r| &out[r]);
            let re = Rx::new(&format!(r"<{p}{item}\b[^>]*?(?:/>|>[\s\S]*?</{p}{item}>)"))?;
            let items: Vec<String> = re.find_all(inner).iter().take(base).map(|x| inner[x.start()..x.end()].to_owned()).collect();
            Ok(Some(Block { items, at: m.start(), len: m.end() - m.start() }))
        };
        let order =
            ["numFmts", "fonts", "fills", "borders", "cellStyleXfs", "cellXfs", "cellStyles", "dxfs", "tableStyles", "colors", "extLst"];
        let put = |out: &mut String,
                   name: &str,
                   item: &str,
                   base: usize,
                   extra: &[String],
                   fallback: &[String],
                   defaults: usize|
         -> Result<(), SaveError> {
            if extra.is_empty() {
                return Ok(());
            }
            let b = block(out, name, item, base)?;
            let items: Vec<String> = match &b {
                Some(b) => b.items.clone(),
                None => fallback.iter().take(defaults).cloned().collect(),
            };
            let xml_block = format!("<{p}{name} count=\"{}\">{}{}</{p}{name}>", items.len() + extra.len(), items.concat(), extra.concat());
            if let Some(b) = b {
                *out = format!("{}{xml_block}{}", &out[..b.at], &out[b.at + b.len..]);
                return Ok(());
            }
            let pos = order.iter().position(|n| *n == name).map_or(0, |i| i + 1);
            let mut at: Option<usize> = None;
            for n in &order[pos..] {
                if let Some(m) = Rx::new(&format!(r"<{p}{n}[\s/>]"))?.find(out, 0) {
                    at = Some(m.start());
                    break;
                }
            }
            let at = at.or_else(|| out.rfind(&format!("</{p}styleSheet>")));
            if let Some(at) = at {
                *out = format!("{}{xml_block}{}", &out[..at], &out[at..]);
            }
            Ok(())
        };
        let font_items = block(&out, "fonts", "font", self.font_base)?.map(|b| b.items).unwrap_or_default();
        let font_extra: Vec<String> = self
            .fonts
            .iter()
            .map(|a| font_xml(font_items.get(a.tpl).map(String::as_str), styles.fonts.get(a.tpl), &a.font, &p))
            .collect::<Result<_, _>>()?;
        put(
            &mut out,
            "fonts",
            "font",
            self.font_base,
            &font_extra,
            &[format!("<{p}font><{p}sz val=\"11\"/><{p}name val=\"Calibri\"/></{p}font>")],
            self.defaults.0,
        )?;
        let fill_extra: Vec<String> = self.fills.iter().map(|f| fill_xml(f, &p)).collect();
        put(
            &mut out,
            "fills",
            "fill",
            self.fill_base,
            &fill_extra,
            &[
                format!("<{p}fill><{p}patternFill patternType=\"none\"/></{p}fill>"),
                format!("<{p}fill><{p}patternFill patternType=\"gray125\"/></{p}fill>"),
            ],
            self.defaults.1,
        )?;
        let border_extra: Vec<String> = self.borders.iter().map(|b| border_xml(b, &p)).collect();
        put(
            &mut out,
            "borders",
            "border",
            self.border_base,
            &border_extra,
            &[format!("<{p}border><{p}left/><{p}right/><{p}top/><{p}bottom/><{p}diagonal/></{p}border>")],
            self.defaults.2,
        )?;
        if !self.added.is_empty() {
            let Some(b) = block(&out, "cellXfs", "xf", self.base)? else { return Ok(xml.to_owned()) };
            let def = format!("<{p}xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>");
            let mut extra: Vec<String> = Vec::with_capacity(self.added.len());
            for a in &self.added {
                let tpl = b.items.get(a.src).or_else(|| b.items.first()).map_or(def.as_str(), String::as_str);
                extra.push(rewrite_xf(tpl, styles.cell_xfs.get(a.src), &a.rec, &p)?);
            }
            out = format!(
                "{}<{p}cellXfs count=\"{}\">{}{}</{p}cellXfs>{}",
                &out[..b.at],
                b.items.len() + extra.len(),
                b.items.concat(),
                extra.concat(),
                &out[b.at + b.len..]
            );
        }
        let mut fmts: Vec<&(f64, String)> = Vec::new();
        for f in &self.fmts {
            let re = Rx::new(&format!(r#"<(?:\w+:)?numFmt\b[^>]*\bnumFmtId="{}""#, num_text(f.0)))?;
            if re.find(&out, 0).is_none() {
                fmts.push(f);
            }
        }
        if !fmts.is_empty() {
            let add: String = fmts
                .iter()
                .map(|(id, code)| format!("<{p}numFmt numFmtId=\"{}\" formatCode=\"{}\"/>", num_text(*id), esc_attr(code)))
                .collect();
            if let Some(nf) = Rx::new(&format!(r"<{p}numFmts\b([^>]*?)(/?)>"))?.find(&out, 0) {
                let self_closing = nf.group(2).is_some_and(|r| !r.is_empty());
                if self_closing {
                    out = format!("{}<{p}numFmts count=\"{}\">{add}</{p}numFmts>{}", &out[..nf.start()], fmts.len(), &out[nf.end()..]);
                } else {
                    let Some(e) = out[nf.start()..].find(&format!("</{p}numFmts>")).map(|i| nf.start() + i) else {
                        return Err(SaveError::Frame("unclosed <numFmts>"));
                    };
                    let body = &out[nf.end()..e.max(nf.end())];
                    let n = Rx::new(&format!(r"<{p}numFmt\b"))?.find_all(body).len() + fmts.len();
                    out = format!("{}<{p}numFmts count=\"{n}\">{body}{add}{}", &out[..nf.start()], &out[e..]);
                }
            } else if let Some(ss) = Rx::new(&format!(r"<{p}styleSheet\b[^>]*>"))?.find(&out, 0) {
                out = format!("{}<{p}numFmts count=\"{}\">{add}</{p}numFmts>{}", &out[..ss.end()], fmts.len(), &out[ss.end()..]);
            }
        }
        Ok(out)
    }
}

/// `Map.set` on an ordered list.
fn set_fmt(v: &mut Vec<(f64, String)>, id: f64, code: &str) {
    match v.iter_mut().find(|(k, _)| *k == id) {
        Some(e) => e.1 = code.to_owned(),
        None => v.push((id, code.to_owned())),
    }
}

/// `setAttr(open, name, value)`: replaces, appends (at the end) or removes the attribute.
fn set_attr(open: &str, name: &str, value: Option<&str>) -> Result<String, SaveError> {
    let re = Rx::new(&format!(r#"\s{name}="[^"]*""#))?;
    let found = re.find(open, 0);
    Ok(match (value, found) {
        (None, Some(m)) => format!("{}{}", &open[..m.start()], &open[m.end()..]),
        (None, None) => open.to_owned(),
        (Some(v), Some(m)) => format!("{} {name}=\"{v}\"{}", &open[..m.start()], &open[m.end()..]),
        (Some(v), None) => format!("{open} {name}=\"{v}\""),
    })
}

/// `<font>` new: the template font's XML with only the elements that CHANGED replaced (theme color, family,
/// charset and scheme stay when unchanged; a new name drops the `<scheme>`, or Excel would use the theme font).
fn font_xml(tpl_xml: Option<&str>, tpl: Option<&Font>, f: &Font, p: &str) -> Result<String, SaveError> {
    let default = Font::default();
    let t = tpl.unwrap_or(&default);
    let mut body = match tpl_xml {
        None => String::new(),
        Some(x) => {
            if Rx::new(r"^<[^>]*/>$")?.find(x, 0).is_some() {
                String::new()
            } else {
                let no_open = match Rx::new(r"^<[^>]*>")?.find(x, 0) {
                    Some(m) => x[m.end()..].to_owned(),
                    None => x.to_owned(),
                };
                match Rx::new(&format!(r"</{p}font>$"))?.find(&no_open, 0) {
                    Some(m) => format!("{}{}", &no_open[..m.start()], &no_open[m.end()..]),
                    None => no_open,
                }
            }
        }
    };
    let drop = |names: &[&str], body: &mut String| -> Result<(), SaveError> {
        for n in names {
            let re = Rx::new(&format!(r"<{p}{n}\b[^>]*?(?:/>|>[\s\S]*?</{p}{n}>)"))?;
            let mut out = String::with_capacity(body.len());
            let mut pos = 0;
            for m in re.find_all(body) {
                out.push_str(&body[pos..m.start()]);
                pos = m.end();
            }
            out.push_str(&body[pos..]);
            *body = out;
        }
        Ok(())
    };
    let has_tpl = tpl_xml.is_some();
    let mut add: Vec<String> = Vec::new();
    for (tv, fv, tag) in [(t.bold, f.bold, "b"), (t.italic, f.italic, "i"), (t.strike, f.strike, "strike")] {
        if !has_tpl || tv != fv {
            drop(&[tag], &mut body)?;
            if fv {
                add.push(format!("<{p}{tag}/>"));
            }
        }
    }
    if !has_tpl || t.underline != f.underline || t.underline_style != f.underline_style {
        drop(&["u"], &mut body)?;
        if f.underline {
            add.push(match f.underline_style {
                Some(s) => format!("<{p}u val=\"{s}\"/>"),
                None => format!("<{p}u/>"),
            });
        }
    }
    if !has_tpl || t.size != f.size {
        drop(&["sz"], &mut body)?;
        if let Some(s) = f.size {
            add.push(format!("<{p}sz val=\"{}\"/>", num_text(s)));
        }
    }
    if !has_tpl || !same_color(t.color.as_deref(), f.color.as_deref()) {
        drop(&["color"], &mut body)?;
        if let Some(c) = f.color.as_deref().filter(|c| !c.is_empty()) {
            add.push(format!("<{p}color rgb=\"{}\"/>", argb(c)));
        }
    }
    if !has_tpl || t.name != f.name {
        drop(&["name", "scheme"], &mut body)?;
        if let Some(n) = f.name.as_deref().filter(|n| !n.is_empty()) {
            add.push(format!("<{p}name val=\"{}\"/>", esc_attr(n)));
        }
    }
    // CT_Font is an unbounded choice: the order of the children does not matter
    Ok(format!("<{p}font>{}{body}</{p}font>", add.concat()))
}

fn fill_xml(f: &Fill, p: &str) -> String {
    format!(
        "<{p}fill><{p}patternFill patternType=\"solid\"><{p}fgColor rgb=\"{}\"/><{p}bgColor indexed=\"64\"/></{p}patternFill></{p}fill>",
        argb(f.fg_color.as_deref().unwrap_or("#000000"))
    )
}

fn border_xml(b: &Border, p: &str) -> String {
    let edge = |k: &str, e: &Option<BorderEdge>| match e {
        Some(e) if !e.style.is_empty() => format!("<{p}{k} style=\"{}\"><{p}color rgb=\"{}\"/></{p}{k}>", e.style, argb(&e.color)),
        _ => format!("<{p}{k}/>"),
    };
    format!(
        "<{p}border>{}{}{}{}{}</{p}border>",
        edge("left", &b.left),
        edge("right", &b.right),
        edge("top", &b.top),
        edge("bottom", &b.bottom),
        edge("diagonal", &b.diagonal)
    )
}

/// `<alignment>` of an xf (no attribute: nothing).
fn alignment_xml(x: &CellXf, p: &str) -> String {
    let mut a = String::new();
    if let Some(h) = x.horizontal.as_deref().filter(|h| !h.is_empty()) {
        a.push_str(&format!(" horizontal=\"{h}\""));
    }
    if let Some(v) = x.vertical.as_deref().filter(|v| !v.is_empty()) {
        a.push_str(&format!(" vertical=\"{v}\""));
    }
    if let Some(r) = x.text_rotation.filter(|r| *r != 0.0 && !r.is_nan()) {
        a.push_str(&format!(" textRotation=\"{}\"", num_text(r)));
    }
    if x.wrap_text {
        a.push_str(" wrapText=\"1\"");
    }
    if let Some(i) = x.indent.filter(|i| *i != 0.0 && !i.is_nan()) {
        a.push_str(&format!(" indent=\"{}\"", num_text(i)));
    }
    if x.shrink_to_fit {
        a.push_str(" shrinkToFit=\"1\"");
    }
    if let Some(r) = x.reading_order.filter(|r| *r != 0.0 && !r.is_nan()) {
        a.push_str(&format!(" readingOrder=\"{}\"", num_text(r)));
    }
    if x.justify_last_line {
        a.push_str(" justifyLastLine=\"1\"");
    }
    if a.is_empty() { String::new() } else { format!("<{p}alignment{a}/>") }
}

/// The source `<xf>` with what changed (ids, applyX, quotePrefix, `<alignment>`).
fn rewrite_xf(xml: &str, src: Option<&CellXf>, x: &PoolXf, p: &str) -> Result<String, SaveError> {
    let end = Rx::new(r"/?>")?.find(xml, 0).map_or(xml.len(), |m| m.start());
    let mut open = xml[..end].to_owned();
    let mut rest = xml[end..].to_owned();
    let default = CellXf::default();
    let s = src.unwrap_or(&default);
    let xf = &x.xf;
    open = set_attr(&open, "numFmtId", Some(&num_text(xf.num_fmt_id)))?;
    if xf.font_id != s.font_id {
        open = set_attr(&open, "fontId", Some(&num_text(xf.font_id)))?;
    }
    if xf.fill_id != s.fill_id {
        open = set_attr(&open, "fillId", Some(&num_text(xf.fill_id)))?;
    }
    if xf.border_id != s.border_id {
        open = set_attr(&open, "borderId", Some(&num_text(xf.border_id)))?;
    }
    if xf.apply_number_format && xf.num_fmt_id != s.num_fmt_id {
        open = set_attr(&open, "applyNumberFormat", Some("1"))?;
    }
    if xf.font_id != s.font_id {
        open = set_attr(&open, "applyFont", Some("1"))?;
    }
    if xf.fill_id != s.fill_id {
        open = set_attr(&open, "applyFill", Some("1"))?;
    }
    if xf.border_id != s.border_id {
        open = set_attr(&open, "applyBorder", Some("1"))?;
    }
    open = set_attr(&open, "quotePrefix", if x.quote_prefix { Some("1") } else { None })?;
    let align_changed = s.horizontal != xf.horizontal
        || s.vertical != xf.vertical
        || s.wrap_text != xf.wrap_text
        || s.indent.unwrap_or(0.0) != xf.indent.unwrap_or(0.0);
    if align_changed {
        open = set_attr(&open, "applyAlignment", Some("1"))?;
        let al = alignment_xml(xf, p);
        let self_closing = rest.starts_with("/>");
        let mut body = if self_closing {
            String::new()
        } else {
            let r1 = &rest[1..];
            match Rx::new(&format!(r"</{p}xf>$"))?.find(r1, 0) {
                Some(m) => format!("{}{}", &r1[..m.start()], &r1[m.end()..]),
                None => r1.to_owned(),
            }
        };
        if let Some(m) = Rx::new(&format!(r"<{p}alignment\b[^>]*?(?:/>|>[\s\S]*?</{p}alignment>)"))?.find(&body, 0) {
            body = format!("{}{}", &body[..m.start()], &body[m.end()..]);
        }
        // `<alignment>` is the first child of CT_Xf
        body = format!("{al}{body}");
        rest = if body.is_empty() { "/>".to_owned() } else { format!(">{body}</{p}xf>") };
    }
    Ok(format!("{open}{rest}"))
}
