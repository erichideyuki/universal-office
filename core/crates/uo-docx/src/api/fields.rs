//! C# `DocxFields`: complex fields (`w:fldChar` begin/separate/end + `w:instrText`, nested,
//! across runs) and simple ones (`w:fldSimple`). Nothing is evaluated: replacing a field by its
//! result keeps the text Word cached. One linear scan with a stack of open fields; fields that
//! never close are ignored; past `max_fields_per_story` or `max_field_nesting` the operations
//! fail with the limit before changing the story.

use std::collections::{HashMap, HashSet};

use super::document::DocxDocument;
use super::dom::{Dom, NodeId, n};
use super::error::{Limit, Result};
use super::net;
use super::ooxml::rel_types;

/// Fields kept by the default sanitization of received content.
pub const DEFAULT_ALLOWED_FIELDS: &[&str] = &["PAGE", "NUMPAGES", "HYPERLINK", "TOC", "REF", "PAGEREF", "SEQ"];

/// Fields kept in a document generated from a template.
pub const DEFAULT_OUTPUT_ALLOWED_FIELDS: &[&str] = &[
    "PAGE",
    "NUMPAGES",
    "HYPERLINK",
    "TOC",
    "REF",
    "PAGEREF",
    "SEQ",
    "DATE",
    "TIME",
    "SAVEDATE",
    "CREATEDATE",
    "PRINTDATE",
    "SECTION",
    "SECTIONPAGES",
    "NOTEREF",
    "=",
];

/// URL schemes accepted in hyperlinks by default.
pub const DEFAULT_ALLOWED_SCHEMES: &[&str] = &["http", "https", "mailto"];

/// A field instruction (name = first word, upper case; a formula is `=`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInstruction {
    pub name: String,
    pub text: String,
}

struct Field {
    begin: NodeId,
    begin_index: usize,
    separate: Option<(NodeId, usize)>,
    end: Option<(NodeId, usize)>,
    instruction: String,
}

/// The run children (no `w:rPr`) of the story, in document order, shared by the fields.
struct Scan {
    elements: Vec<NodeId>,
    done: Vec<Field>,
}

impl Field {
    fn instruction_content<'a>(&self, elements: &'a [NodeId]) -> &'a [NodeId] {
        let stop = self.separate.map(|s| s.1).or(self.end.map(|e| e.1)).unwrap_or(self.begin_index + 1);
        elements.get(self.begin_index + 1..stop.max(self.begin_index + 1)).unwrap_or(&[])
    }
}

/// Complex fields in END order (inner before outer).
fn scan(doc: &DocxDocument, root: NodeId, part: &str) -> Result<Scan> {
    let dom = &doc.dom;
    let limits = doc.limits();
    let mut elements = Vec::new();
    let mut done = Vec::new();
    let mut open: Vec<Field> = Vec::new();
    let mut begins: u64 = 0;
    for run in dom.descendants_named(root, n::r) {
        for e in dom.elements(run).collect::<Vec<_>>() {
            if dom.is(e, n::rPr) {
                continue;
            }
            let index = elements.len();
            elements.push(e);
            let fld = if dom.is(e, n::fldChar) { dom.attr(e, n::fldCharType) } else { None };
            match fld {
                Some("begin") => {
                    begins += 1;
                    if begins > u64::from(limits.max_fields_per_story) {
                        return Err(doc.field_limit(Limit::FieldsPerStory, part));
                    }
                    if open.len() >= limits.max_field_nesting as usize {
                        return Err(doc.field_limit(Limit::FieldNesting, part));
                    }
                    open.push(Field { begin: e, begin_index: index, separate: None, end: None, instruction: String::new() });
                }
                Some("separate") => {
                    if let Some(top) = open.last_mut()
                        && top.separate.is_none()
                    {
                        top.separate = Some((e, index));
                    }
                }
                Some("end") => {
                    if let Some(mut closed) = open.pop() {
                        closed.end = Some((e, index));
                        done.push(closed);
                    }
                }
                _ => {
                    if dom.is(e, n::instrText)
                        && let Some(top) = open.last_mut()
                        && top.separate.is_none()
                    {
                        top.instruction.push_str(&dom.value(e));
                    }
                }
            }
        }
    }
    Ok(Scan { elements, done })
}

fn instruction_of(f: &Field) -> FieldInstruction {
    FieldInstruction { name: first_word(&f.instruction), text: f.instruction.clone() }
}

/// The first word of an instruction, upper case (`=` for a formula).
pub fn first_word(instruction: &str) -> String {
    let trimmed = net::trim_start(instruction);
    if trimmed.starts_with('=') {
        return "=".to_owned();
    }
    let end = trimmed.find(|c: char| net::is_white_space_char(c) || c == '\\').unwrap_or(trimmed.len());
    net::to_upper_invariant(trimmed.get(..end).unwrap_or(""))
}

/// Replaces by the cached result every field whose name is not allowed and every HYPERLINK
/// that is neither an internal anchor nor an absolute URL with an allowed scheme; returns how
/// many fields were replaced.
pub fn replace_disallowed_with_result(
    doc: &mut DocxDocument,
    allowed_fields: Option<&[&str]>,
    allowed_schemes: Option<&[&str]>,
) -> Result<usize> {
    let fields: HashSet<String> = allowed_fields.unwrap_or(DEFAULT_ALLOWED_FIELDS).iter().map(|f| net::fold_key(f)).collect();
    let schemes: Vec<String> = allowed_schemes.unwrap_or(DEFAULT_ALLOWED_SCHEMES).iter().map(|s| (*s).to_owned()).collect();
    let mut count = 0;
    for (part, root) in doc.stories(true, true)? {
        count += replace_with_result(doc, root, &part, |i| !is_allowed(i, &fields, &schemes))?;
    }
    Ok(count)
}

/// Replaces by the result the fields for which `replace` is true (removals batched per parent).
pub(crate) fn replace_with_result(
    doc: &mut DocxDocument,
    root: NodeId,
    part: &str,
    replace: impl Fn(&FieldInstruction) -> bool,
) -> Result<usize> {
    let s = scan(doc, root, part)?;
    let mut count = 0;
    let mut markers: Vec<NodeId> = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |e: NodeId, markers: &mut Vec<NodeId>| {
        if seen.insert(e) {
            markers.push(e);
        }
    };
    for f in &s.done {
        let Some((end, _)) = f.end else { continue };
        if !replace(&instruction_of(f)) {
            continue;
        }
        add(f.begin, &mut markers);
        for &e in f.instruction_content(&s.elements) {
            add(e, &mut markers);
        }
        if let Some((sep, _)) = f.separate {
            add(sep, &mut markers);
        }
        add(end, &mut markers);
        count += 1;
    }
    let dom = &mut doc.dom;
    if !markers.is_empty() {
        let mut runs: Vec<NodeId> = Vec::new();
        let mut rs = HashSet::new();
        for &m in &markers {
            if let Some(p) = dom.parent(m)
                && rs.insert(p)
            {
                runs.push(p);
            }
        }
        dom.remove_all(&markers);
        let empty: Vec<NodeId> = runs
            .into_iter()
            .filter(|&r| dom.is(r, n::r) && dom.parent(r).is_some() && dom.elements(r).all(|e| dom.is(e, n::rPr)))
            .collect();
        dom.remove_all(&empty);
    }
    let mut simples = Vec::new();
    for simple in dom.descendants_named(root, n::fldSimple) {
        let instr = dom.attr(simple, n::instr).unwrap_or("").to_owned();
        if replace(&FieldInstruction { name: first_word(&instr), text: instr }) {
            simples.push(simple);
        }
    }
    dom.unwrap_all(&simples);
    Ok(count + simples.len())
}

fn is_allowed(i: &FieldInstruction, fields: &HashSet<String>, schemes: &[String]) -> bool {
    if !fields.contains(&net::fold_key(&i.name)) {
        return false;
    }
    if !net::eq_ignore_case(&i.name, "HYPERLINK") {
        return true;
    }
    let sch: Vec<&str> = schemes.iter().map(String::as_str).collect();
    try_parse_hyperlink(&i.text, &sch).is_some()
}

/// A parsed HYPERLINK target: an absolute URL (with `#location`) or an internal anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HyperlinkTarget {
    Url(String),
    Anchor(String),
}

/// Parses the HYPERLINK instruction: an internal anchor (`\l "x"`) or an absolute URL with an
/// allowed scheme; `None` for anything else.
pub fn try_parse_hyperlink(instruction: &str, allowed_schemes: &[&str]) -> Option<HyperlinkTarget> {
    let tokens = tokenize(instruction);
    if !tokens.first().is_some_and(|t| net::eq_ignore_case(t, "HYPERLINK")) {
        return None;
    }
    let mut target: Option<String> = None;
    let mut location: Option<String> = None;
    let mut k = 1;
    while k < tokens.len() {
        let token = tokens.get(k).cloned().unwrap_or_default();
        let u = net::units(&token);
        if u.len() == 2 && u.first() == Some(&0x5C) {
            let sw = u.get(1).copied().unwrap_or(0);
            let sw = if (0x41..=0x5A).contains(&sw) { sw + 32 } else { sw };
            if sw == u16::from(b'l') && k + 1 < tokens.len() {
                k += 1;
                location = tokens.get(k).cloned();
            } else if (sw == u16::from(b'o') || sw == u16::from(b't')) && k + 1 < tokens.len() {
                k += 1;
            }
            k += 1;
            continue;
        }
        if target.is_none() {
            target = Some(token);
        }
        k += 1;
    }
    match target {
        None => match location {
            Some(l) if !l.is_empty() => Some(HyperlinkTarget::Anchor(l)),
            _ => None,
        },
        Some(t) => {
            if !is_allowed_url(&t, allowed_schemes) {
                return None;
            }
            Some(HyperlinkTarget::Url(match location {
                Some(l) if !l.is_empty() => format!("{t}#{l}"),
                _ => t,
            }))
        }
    }
}

/// The words of an instruction: quoted texts (without the quotes) and runs of other characters.
fn tokenize(instruction: &str) -> Vec<String> {
    let u = net::units(instruction);
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < u.len() {
        let c = u.get(i).copied().unwrap_or(0);
        if net::is_white_space(c) {
            i += 1;
            continue;
        }
        if c == 0x22 {
            let close = u.iter().skip(i + 1).position(|&x| x == 0x22).map_or(u.len(), |k| i + 1 + k);
            tokens.push(net::from_units(u.get(i + 1..close).unwrap_or(&[])));
            i = close + 1;
            continue;
        }
        let start = i;
        while i < u.len() && u.get(i).is_some_and(|&x| !net::is_white_space(x) && x != 0x22) {
            i += 1;
        }
        tokens.push(net::from_units(u.get(start..i).unwrap_or(&[])));
    }
    tokens
}

/// An absolute URL whose scheme is in the list (`Uri.TryCreate(…, Absolute)`, OrdinalIgnoreCase).
pub fn is_allowed_url(url: &str, allowed_schemes: &[&str]) -> bool {
    net::absolute_uri_scheme(url).is_some_and(|s| allowed_schemes.iter().any(|a| net::eq_ignore_case(a, &s)))
}

/// Run made only of `w:rPr` + `w:fldChar` (+ `w:instrText` when allowed).
fn is_marker_run(dom: &Dom, run: NodeId, allow_instruction: bool) -> bool {
    dom.is(run, n::r)
        && dom.elements(run).all(|e| dom.is(e, n::rPr) || dom.is(e, n::fldChar) || (allow_instruction && dom.is(e, n::instrText)))
}

/// New relationship ids above the existing ones, without re-reading the `.rels`.
struct RelationshipIds {
    used: HashSet<String>,
    max: i64,
}

impl RelationshipIds {
    fn new(existing: impl Iterator<Item = String>) -> Self {
        let used: HashSet<String> = existing.collect();
        let mut max = 0i64;
        for id in &used {
            if let Some(rest) = id.strip_prefix("rId")
                && let Some(v) = net::parse_int_digits(rest)
            {
                max = max.max(i64::from(v));
            }
        }
        RelationshipIds { used, max }
    }

    fn next(&mut self) -> String {
        loop {
            self.max += 1;
            let c = format!("rId{}", self.max);
            if self.used.insert(c.clone()) {
                return c;
            }
        }
    }
}

/// Converts allowed HYPERLINK fields into native `w:hyperlink` (external relationship or
/// `w:anchor`) when the whole field is in runs that are direct children of one paragraph;
/// returns how many. Each paragraph is rebuilt once.
pub fn convert_hyperlink_fields(doc: &mut DocxDocument, allowed_schemes: Option<&[&str]>) -> Result<usize> {
    let schemes: Vec<&str> = allowed_schemes.unwrap_or(DEFAULT_ALLOWED_SCHEMES).to_vec();
    let mut count = 0;
    for (part, root) in doc.stories(true, true)? {
        let s = scan(doc, root, &part)?;
        let mut consumed: HashSet<NodeId> = HashSet::new();
        let mut hyperlinks: HashMap<NodeId, (NodeId, Vec<NodeId>)> = HashMap::new();
        let mut paragraphs: Vec<NodeId> = Vec::new();
        let mut paragraph_set = HashSet::new();
        let mut ids: Option<RelationshipIds> = None;
        for f in &s.done {
            let dom = &doc.dom;
            let (Some((end, _)), Some((sep, _))) = (f.end, f.separate) else { continue };
            let (Some(end_run), Some(sep_run), Some(begin_run)) = (dom.parent(end), dom.parent(sep), dom.parent(f.begin)) else { continue };
            let instruction = instruction_of(f);
            if !net::eq_ignore_case(&instruction.name, "HYPERLINK") {
                continue;
            }
            let Some(target) = try_parse_hyperlink(&instruction.text, &schemes) else { continue };
            let Some(paragraph) = dom.parent(begin_run) else { continue };
            if !dom.is(paragraph, n::p) || dom.parent(sep_run) != Some(paragraph) || dom.parent(end_run) != Some(paragraph) {
                continue;
            }
            if !is_marker_run(dom, begin_run, true) || !is_marker_run(dom, sep_run, false) || !is_marker_run(dom, end_run, false) {
                continue;
            }
            let mut span = vec![begin_run];
            let (mut si, mut ei) = (None, None);
            for sib in dom.following_elements(begin_run) {
                if ei.is_some() {
                    break;
                }
                span.push(sib);
                if sib == sep_run && si.is_none() {
                    si = Some(span.len() - 1);
                }
                if sib == end_run {
                    ei = Some(span.len() - 1);
                }
            }
            let (Some(si), Some(ei)) = (si, ei) else { continue };
            if !(0 < si && si < ei) {
                continue;
            }
            if span.iter().skip(1).take(si - 1).any(|&x| !dom.is(x, n::r) || !is_marker_run(dom, x, true)) {
                continue;
            }
            let result: Vec<NodeId> = span.get(si + 1..ei).unwrap_or(&[]).to_vec();
            if result.iter().any(|&x| dom.first_descendant(x, |d, e| d.is(e, n::fldChar)).is_some()) {
                continue;
            }
            if span.iter().any(|x| consumed.contains(x)) {
                continue;
            }
            let hyperlink = doc.dom.new_element(n::hyperlink);
            match target {
                HyperlinkTarget::Anchor(a) => doc.dom.set_attr(hyperlink, n::anchor, Some(&a)),
                HyperlinkTarget::Url(url) => {
                    if ids.is_none() {
                        ids = Some(RelationshipIds::new(doc.get_relationships(&part)?.into_iter().map(|r| r.id)));
                    }
                    let id = ids.as_mut().map(RelationshipIds::next).unwrap_or_default();
                    doc.add_relationship_with_id(&part, &id, rel_types::HYPERLINK, &url, true)?;
                    doc.dom.set_attr(hyperlink, n::r_id, Some(&id));
                }
            }
            doc.dom.set_attr(hyperlink, n::history, Some("1"));
            consumed.extend(span.iter().copied());
            if paragraph_set.insert(paragraph) {
                paragraphs.push(paragraph);
            }
            hyperlinks.insert(begin_run, (hyperlink, result));
            count += 1;
        }
        let dom = &mut doc.dom;
        for paragraph in paragraphs {
            let nodes: Vec<NodeId> = dom.children(paragraph).to_vec();
            dom.remove_nodes(paragraph);
            let mut rebuilt = Vec::with_capacity(nodes.len());
            for node in nodes {
                if dom.is_element(node) {
                    if let Some((h, result)) = hyperlinks.get(&node) {
                        dom.append_all(*h, result);
                        rebuilt.push(*h);
                        continue;
                    }
                    if consumed.contains(&node) {
                        continue;
                    }
                }
                rebuilt.push(node);
            }
            dom.append_all(paragraph, &rebuilt);
        }
    }
    Ok(count)
}

/// The instructions of every field (complex and simple) of the story parts.
pub fn get_instructions(doc: &mut DocxDocument) -> Result<Vec<FieldInstruction>> {
    let mut list = Vec::new();
    for (part, root) in doc.stories(true, true)? {
        let s = scan(doc, root, &part)?;
        list.extend(s.done.iter().map(instruction_of));
        let dom = &doc.dom;
        for simple in dom.descendants_named(root, n::fldSimple) {
            let i = dom.attr(simple, n::instr).unwrap_or("").to_owned();
            list.push(FieldInstruction { name: first_word(&i), text: i });
        }
    }
    Ok(list)
}
