//! The regular expressions of the drawing family (D2b-F7), written as the matchers they became.
//! Every function says which TS expression it is; `tests/regex_d2b_f7.rs` compares each one with
//! V8 over `tests/regex/d2b-f7.inputs.txt` (rows in `js/regex-sites/d2b-f7.mjs`). The patterns
//! are ASCII, so the scans run on bytes; `\b` is the ASCII word boundary (no `u` flag).

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `\b` after a literal that ends with a word character: the next byte is not a word byte.
fn wb_after(s: &str, pos: usize) -> bool {
    s.as_bytes().get(pos).is_none_or(|b| !is_word(*b))
}

/// `\b` before a literal that starts with a word character at `pos`.
fn wb_before(s: &str, pos: usize) -> bool {
    pos == 0 || s.as_bytes().get(pos - 1).is_none_or(|b| !is_word(*b))
}

fn find_from(s: &str, pat: &str, from: usize) -> Option<usize> {
    s.get(from..)?.find(pat).map(|i| i + from)
}

/// The digits run at `at` and whether a `"` follows it: the end of `\d+"` (`None` when it fails).
fn digits_quote(s: &str, at: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut e = at;
    while b.get(e).is_some_and(u8::is_ascii_digit) {
        e += 1;
    }
    (e > at && b.get(e) == Some(&b'"')).then_some(e)
}

fn cut(s: &str, i: usize, e: usize) -> String {
    format!("{}{}", s.get(..i).unwrap_or(""), s.get(e..).unwrap_or(""))
}

/// `s.replace(/<a:xfrm\b[\s\S]*?<\/a:xfrm>|<a:xfrm\/>/, '')`
#[doc(hidden)]
#[must_use]
pub fn strip_xfrm(s: &str) -> String {
    let mut from = 0usize;
    while let Some(i) = find_from(s, "<a:xfrm", from) {
        let n = i + "<a:xfrm".len();
        if wb_after(s, n)
            && let Some(j) = find_from(s, "</a:xfrm>", n)
        {
            return cut(s, i, j + "</a:xfrm>".len());
        }
        if s.get(i..).is_some_and(|t| t.starts_with("<a:xfrm/>")) {
            return cut(s, i, i + "<a:xfrm/>".len());
        }
        from = i + 1;
    }
    s.to_owned()
}

/// `s.replace(/<a:prstGeom\b[\s\S]*?<\/a:prstGeom>|<a:prstGeom\b[^>]*\/>|<a:custGeom\b[\s\S]*?<\/a:custGeom>/, '')`
#[doc(hidden)]
#[must_use]
pub fn strip_geometry(s: &str) -> String {
    let mut from = 0usize;
    loop {
        let p = find_from(s, "<a:prstGeom", from);
        let c = find_from(s, "<a:custGeom", from);
        let i = match (p, c) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => return s.to_owned(),
        };
        let n = i + "<a:prstGeom".len();
        if s.get(i..).is_some_and(|t| t.starts_with("<a:prstGeom")) && wb_after(s, n) {
            if let Some(j) = find_from(s, "</a:prstGeom>", n) {
                return cut(s, i, j + "</a:prstGeom>".len());
            }
            // `[^>]*\/>`: the first `>` after the name, preceded by `/` that is not before the name
            if let Some(g) = find_from(s, ">", n)
                && g > n
                && s.as_bytes().get(g - 1) == Some(&b'/')
            {
                return cut(s, i, g + 1);
            }
        }
        if s.get(i..).is_some_and(|t| t.starts_with("<a:custGeom"))
            && wb_after(s, n)
            && let Some(j) = find_from(s, "</a:custGeom>", n)
        {
            return cut(s, i, j + "</a:custGeom>".len());
        }
        from = i + 1;
    }
}

/// `/<wps:spPr\b[^>]*>([\s\S]*?)<\/wps:spPr>/.exec(raw)?.[1] ?? ''`
#[doc(hidden)]
#[must_use]
pub fn wps_sp_pr(raw: &str) -> String {
    let mut from = 0usize;
    while let Some(i) = find_from(raw, "<wps:spPr", from) {
        let n = i + "<wps:spPr".len();
        if wb_after(raw, n) {
            let Some(g) = find_from(raw, ">", n) else { return String::new() };
            return match find_from(raw, "</wps:spPr>", g + 1) {
                Some(j) => raw.get(g + 1..j).unwrap_or("").to_owned(),
                None => String::new(),
            };
        }
        from = i + 1;
    }
    String::new()
}

/// The `spIn` of `setAsDefaultTextbox`: the `spPr` of the box without `a:xfrm` and the geometry.
#[doc(hidden)]
#[must_use]
pub fn sp_pr_inner(raw: &str) -> String {
    strip_geometry(&strip_xfrm(&wps_sp_pr(raw)))
}

/// `xml.replace(/<a:ext uri="\{96DAC541-7B7A-43D3-8B79-37D633B846F1\}">[\s\S]*?<\/a:ext>/g, '')`
#[doc(hidden)]
#[must_use]
pub fn strip_svg_blip_ext(raw: &str) -> String {
    const OPEN: &str = "<a:ext uri=\"{96DAC541-7B7A-43D3-8B79-37D633B846F1}\">";
    const CLOSE: &str = "</a:ext>";
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(i) = rest.find(OPEN) {
        let after = rest.get(i + OPEN.len()..).unwrap_or("");
        let Some(j) = after.find(CLOSE) else { break };
        out.push_str(rest.get(..i).unwrap_or(""));
        rest = after.get(j + CLOSE.len()..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// `/^glow-(\d+)-(\d)$/.exec(preset)`: the size index and the accent digit (`$` is the end of the
/// string here: no `m` flag).
#[doc(hidden)]
#[must_use]
pub fn parse_glow(p: &str) -> Option<(f64, f64)> {
    let rest = p.strip_prefix("glow-")?;
    let (a, b) = rest.split_once('-')?;
    if a.is_empty() || !a.bytes().all(|c| c.is_ascii_digit()) || b.len() != 1 || !b.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((uo_jsnum::parse::to_number(a), uo_jsnum::parse::to_number(b)))
}

/// The `[start, end)` of the first `(<wp:docPr\b[^>]*?\bid=")\d+"` after `from`: where the digits
/// begin and where the closing quote is.
fn doc_pr_id_at(s: &str, from: usize) -> Option<(usize, usize)> {
    let mut f = from;
    while let Some(i) = find_from(s, "<wp:docPr", f) {
        let n = i + "<wp:docPr".len();
        if wb_after(s, n) {
            let limit = find_from(s, ">", n).unwrap_or(s.len());
            let mut j = n;
            while j < limit {
                if s.get(j..).is_some_and(|t| t.starts_with("id=\"")) && wb_before(s, j) {
                    let v = j + 4;
                    if let Some(e) = digits_quote(s, v) {
                        return Some((v, e));
                    }
                }
                j += 1;
            }
        }
        f = i + 1;
    }
    None
}

/// Every `id` of `/<wp:docPr\b[^>]*?\bid="(\d+)"/g` in `raw`, as the digit text.
#[doc(hidden)]
#[must_use]
pub fn doc_pr_ids(raw: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some((v, e)) = doc_pr_id_at(raw, from) {
        if let Some(d) = raw.get(v..e) {
            out.push(d);
        }
        from = e + 1;
    }
    out
}

/// `xml.replace(/(<wp:docPr\b[^>]*?\bid=")\d+"/, (_m, a) => `${a}${new}"`)`
#[doc(hidden)]
#[must_use]
pub fn replace_doc_pr_id(xml: &str, new: &str) -> String {
    match doc_pr_id_at(xml, 0) {
        Some((v, e)) => format!("{}{new}{}", xml.get(..v).unwrap_or(""), xml.get(e..).unwrap_or("")),
        None => xml.to_owned(),
    }
}

/// `xml.replace(/(<wp:docPr\b[^>]*?\bname=")[^"]*"/, (_m, a) => `${a}${new}"`)`
#[doc(hidden)]
#[must_use]
pub fn replace_doc_pr_name(xml: &str, new: &str) -> String {
    let mut f = 0usize;
    while let Some(i) = find_from(xml, "<wp:docPr", f) {
        let n = i + "<wp:docPr".len();
        if wb_after(xml, n) {
            let limit = find_from(xml, ">", n).unwrap_or(xml.len());
            let mut j = n;
            while j < limit {
                if xml.get(j..).is_some_and(|t| t.starts_with("name=\"")) && wb_before(xml, j) {
                    let v = j + 6;
                    if let Some(q) = find_from(xml, "\"", v) {
                        return format!("{}{new}{}", xml.get(..v).unwrap_or(""), xml.get(q..).unwrap_or(""));
                    }
                }
                j += 1;
            }
        }
        f = i + 1;
    }
    xml.to_owned()
}

/// `xml.replace(/\brelativeHeight="\d+"/, () => `relativeHeight="${new}"`)`
#[doc(hidden)]
#[must_use]
pub fn replace_relative_height(xml: &str, new: &str) -> String {
    const PAT: &str = "relativeHeight=\"";
    let mut f = 0usize;
    while let Some(j) = find_from(xml, PAT, f) {
        if wb_before(xml, j)
            && let Some(e) = digits_quote(xml, j + PAT.len())
        {
            return format!("{}relativeHeight=\"{new}\"{}", xml.get(..j).unwrap_or(""), xml.get(e + 1..).unwrap_or(""));
        }
        f = j + 1;
    }
    xml.to_owned()
}

/// `themePartPath()`: `/<Relationship\b[^>]*Type="[^"]*\/theme"[^>]*>/` on the rels, then
/// `/Target="([^"]+)"/` on the match, then the two `replace`s; the part name under `word/`.
#[doc(hidden)]
#[must_use]
pub fn theme_part_path(rels: &str) -> Option<String> {
    let m = relationship_to_theme(rels)?;
    let t = target_of(m)?;
    let t = t.strip_prefix("/word/").or_else(|| t.strip_prefix("word/")).unwrap_or(t);
    let t = t.strip_prefix("./").unwrap_or(t);
    Some(format!("word/{t}"))
}

fn relationship_to_theme(rels: &str) -> Option<&str> {
    const TYPE: &str = "Type=\"";
    let mut f = 0usize;
    while let Some(i) = find_from(rels, "<Relationship", f) {
        let n = i + "<Relationship".len();
        if wb_after(rels, n) {
            let limit = find_from(rels, ">", n).unwrap_or(rels.len());
            // the greedy `[^>]*` leaves the last `Type="` that fits
            let mut js: Vec<usize> = Vec::new();
            let mut j = n;
            while let Some(k) = find_from(rels, TYPE, j) {
                if k >= limit {
                    break;
                }
                js.push(k);
                j = k + 1;
            }
            for &k in js.iter().rev() {
                let v = k + TYPE.len();
                let Some(q) = find_from(rels, "\"", v) else { continue };
                if rels.get(v..q).is_some_and(|x| x.ends_with("/theme"))
                    && let Some(e) = find_from(rels, ">", q + 1)
                {
                    return rels.get(i..=e);
                }
            }
        }
        f = i + 1;
    }
    None
}

/// `/Target="([^"]+)"/.exec(m)?.[1]` (nothing keeps the name from being the tail of a longer one)
#[doc(hidden)]
#[must_use]
pub fn target_of(m: &str) -> Option<&str> {
    const PAT: &str = "Target=\"";
    let mut f = 0usize;
    while let Some(j) = find_from(m, PAT, f) {
        let v = j + PAT.len();
        if let Some(q) = find_from(m, "\"", v)
            && q > v
        {
            return m.get(v..q);
        }
        f = j + 1;
    }
    None
}

/// The text between `<a:objectDefaults>` … `<a:txDef>` and its `</a:txDef>`:
/// `/<a:objectDefaults>[\s\S]*?<a:txDef>([\s\S]*?)<\/a:txDef>/.exec(x)?.[1]`
#[doc(hidden)]
#[must_use]
pub fn tx_def(x: &str) -> Option<&str> {
    let od = x.find("<a:objectDefaults>")? + "<a:objectDefaults>".len();
    let s = find_from(x, "<a:txDef>", od)? + "<a:txDef>".len();
    let e = find_from(x, "</a:txDef>", s)?;
    x.get(s..e)
}

/// `/<a:spPr>([\s\S]*?)<\/a:spPr>|<a:spPr\/>/.exec(td)?.[1] ?? ''`
#[doc(hidden)]
#[must_use]
pub fn tx_def_sp_pr(td: &str) -> &str {
    let mut f = 0usize;
    while let Some(i) = find_from(td, "<a:spPr", f) {
        let rest = td.get(i..).unwrap_or("");
        if rest.starts_with("<a:spPr>")
            && let Some(j) = find_from(td, "</a:spPr>", i + "<a:spPr>".len())
        {
            return td.get(i + "<a:spPr>".len()..j).unwrap_or("");
        }
        if rest.starts_with("<a:spPr/>") {
            return "";
        }
        f = i + 1;
    }
    ""
}

/// `/<a:bodyPr\b([^>]*?)\/?>/.exec(td)?.[1] ?? ''`
#[doc(hidden)]
#[must_use]
pub fn tx_def_body_pr(td: &str) -> &str {
    let mut f = 0usize;
    while let Some(i) = find_from(td, "<a:bodyPr", f) {
        let n = i + "<a:bodyPr".len();
        if wb_after(td, n) {
            let Some(g) = find_from(td, ">", n) else { return "" };
            let end = if g > n && td.as_bytes().get(g - 1) == Some(&b'/') { g - 1 } else { g };
            return td.get(n..end).unwrap_or("");
        }
        f = i + 1;
    }
    ""
}

/// `new RegExp(`\\s${n}="([^"]*)"`).exec(bp)?.[1]` for a plain attribute name.
#[doc(hidden)]
#[must_use]
pub fn body_pr_attr<'a>(bp: &'a str, n: &str) -> Option<&'a str> {
    let pat = format!("{n}=\"");
    let mut f = 0usize;
    while let Some(j) = find_from(bp, &pat, f) {
        let prev_space = bp.get(..j).and_then(|h| h.chars().next_back()).is_some_and(uo_jsnum::text::is_js_space);
        if prev_space {
            let v = j + pat.len();
            if let Some(q) = find_from(bp, "\"", v) {
                return bp.get(v..q);
            }
        }
        f = j + 1;
    }
    None
}

/// `/<a:txDef>[\s\S]*?<\/a:txDef>/`: the start and end of the first element.
#[doc(hidden)]
#[must_use]
pub fn tx_def_span(theme: &str) -> Option<(usize, usize)> {
    let i = theme.find("<a:txDef>")?;
    let j = find_from(theme, "</a:txDef>", i + "<a:txDef>".len())?;
    Some((i, j + "</a:txDef>".len()))
}

/// `squash`: `s.replace(/>\s+</g, '><').replace(/\s+/g, ' ')`.
#[doc(hidden)]
#[must_use]
pub fn squash(s: &str) -> String {
    use uo_jsnum::text::is_js_space;
    // `>\s+<` → `><`
    let cs: Vec<char> = s.chars().collect();
    let mut a = String::with_capacity(s.len());
    let mut i = 0usize;
    while let Some(&c) = cs.get(i) {
        if c == '>' {
            let mut j = i + 1;
            while cs.get(j).is_some_and(|x| is_js_space(*x)) {
                j += 1;
            }
            if j > i + 1 && cs.get(j) == Some(&'<') {
                a.push_str("><");
                i = j + 1;
                continue;
            }
        }
        a.push(c);
        i += 1;
    }
    // `\s+` → ` `
    let mut out = String::with_capacity(a.len());
    let mut in_ws = false;
    for c in a.chars() {
        if is_js_space(c) {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            in_ws = false;
            out.push(c);
        }
    }
    out
}

/// What `shapeStyleIndex` compares with the gallery: the squashed `wps:style` and the squashed
/// fill/line part of the `wps:spPr` (after the geometry, before the effects); `None` when the
/// shape has no `wps:style` (its `0`).
#[doc(hidden)]
#[must_use]
pub fn shape_style_parts(raw: &str) -> Option<(String, String)> {
    let i = raw.find("<wps:style>")?;
    let j = find_from(raw, "</wps:style>", i + "<wps:style>".len())?;
    let st = squash(raw.get(i..j + "</wps:style>".len())?);
    let sp = wps_sp_pr(raw);
    // `^[\s\S]*?(<\/a:prstGeom>|<\/a:custGeom>|<a:prstGeom\b[^>]*\/>)` removed
    let mut cut_at = None;
    let mut q = 0usize;
    while let Some(k) = find_from(&sp, "<", q) {
        let t = sp.get(k..).unwrap_or("");
        if t.starts_with("</a:prstGeom>") {
            cut_at = Some(k + "</a:prstGeom>".len());
            break;
        }
        if t.starts_with("</a:custGeom>") {
            cut_at = Some(k + "</a:custGeom>".len());
            break;
        }
        if t.starts_with("<a:prstGeom") {
            let n = k + "<a:prstGeom".len();
            if wb_after(&sp, n)
                && let Some(g) = find_from(&sp, ">", n)
                && g > n
                && sp.as_bytes().get(g - 1) == Some(&b'/')
            {
                cut_at = Some(g + 1);
                break;
            }
        }
        q = k + 1;
    }
    let rest = cut_at.map_or(sp.as_str(), |c| sp.get(c..).unwrap_or(""));
    let mut fill_ln = squash(rest);
    // `<a:effectLst[\s\S]*$|<a:scene3d[\s\S]*$|<a:sp3d[\s\S]*$|<a:extLst[\s\S]*$` (the first one)
    let cut = ["<a:effectLst", "<a:scene3d", "<a:sp3d", "<a:extLst"].iter().filter_map(|p| fill_ln.find(p)).min();
    if let Some(c) = cut {
        fill_ln.truncate(c);
    }
    Some((st, fill_ln))
}

/// `shapeStyleIndex(raw)`: the gallery style (1..=77) the shape carries, or 0.
#[doc(hidden)]
#[must_use]
pub fn shape_style_index(raw: &str) -> u32 {
    shape_style_parts(raw).and_then(|(st, fl)| crate::drawing::shape_styles::find_shape_style(&fl, &st)).unwrap_or(0)
}
