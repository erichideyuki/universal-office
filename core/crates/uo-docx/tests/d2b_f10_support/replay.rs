//! The oracle of the F10 session parity: it answers what the TS layout answered in Chromium
//! (`i-fv`, `i-pg`, `i-mh` of `d2b-f10-session.capture`) and records HOW the session asked:
//! the number of `field_values` calls and the scope of each (`fvs`), whether the page labels were asked
//! and whether the request covers every paragraph the TS asked `pageLabelOf` about (`pgc`, `pgv`), and
//! the forced-hyphenation layout calls (`mhc`).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{HashMap, HashSet, VecDeque};

use uo_docx::session::{ClipboardRing, ExecCx};
use uo_docx::text::Utf16;
use uo_docx::view::{
    BlockPath, DocRef, DocView, FieldScope, HyphenCandidate, HyphenSkips, LayoutError, LayoutErrorKind, LayoutOracle, RunRef, StoryRef,
};
use uo_testkit::Digest;

use super::names::names;

/// One `refreshFieldCache` of the TS: the values `setFieldResult` received, by key.
#[derive(Debug, Clone, Default)]
pub struct FvCall {
    /// the keys of `only` (`None` = every field)
    pub only: Option<Vec<String>>,
    /// how many runs of `only` the capture's walk does not name (stale runs, other stories)
    pub unnamed: usize,
    pub vals: Vec<(String, Utf16)>,
}

/// What the TS did in one step (`i-fv`, `i-pg`, `i-mh`).
#[derive(Debug, Clone, Default)]
pub struct StepScript {
    pub fv: VecDeque<FvCall>,
    /// page label by paragraph ordinal
    pub pg: HashMap<u32, f64>,
    /// the word proposed: paragraph, start, end, limit
    pub mh: Option<(u32, usize, usize, u32)>,
}

fn unhex(h: &str) -> Vec<u16> {
    h.split('.').filter(|x| !x.is_empty()).map(|x| u16::from_str_radix(x, 16).unwrap()).collect()
}

impl StepScript {
    pub fn parse(fv: Option<&str>, pg: Option<&str>, mh: Option<&str>) -> StepScript {
        let mut s = StepScript::default();
        if let Some(fv) = fv.filter(|x| *x != "-") {
            for call in fv.split('|') {
                let (scope, vals) = call.split_once('!').unwrap();
                let (only, unnamed) = match scope.strip_prefix('R') {
                    Some(k) => {
                        let (k, u) = k.split_once('^').unwrap();
                        (Some(k.split(',').filter(|x| !x.is_empty()).map(str::to_owned).collect::<Vec<_>>()), u.parse().unwrap())
                    }
                    None => (None, 0),
                };
                let vals = vals
                    .split(';')
                    .filter(|x| !x.is_empty())
                    .map(|kv| {
                        // a key may hold a ':' (`f1:0`), the hex value never does
                        let (k, v) = kv.rsplit_once(':').unwrap();
                        (k.to_owned(), Utf16::from_units(unhex(v)))
                    })
                    .collect();
                s.fv.push_back(FvCall { only, unnamed, vals });
            }
        }
        if let Some(pg) = pg.filter(|x| *x != "-") {
            for kv in pg.split(',') {
                let (k, v) = kv.split_once(':').unwrap();
                s.pg.insert(k.parse().unwrap(), v.parse().unwrap());
            }
        }
        if let Some(mh) = mh.filter(|x| *x != "-") {
            let v: Vec<&str> = mh.split(':').collect();
            s.mh = Some((v[0].parse().unwrap(), v[1].parse().unwrap(), v[2].parse().unwrap(), v[3].parse().unwrap()));
        }
        s
    }
}

/// The scripted oracle and its record of the step.
#[derive(Default)]
pub struct ReplayOracle {
    pub script: StepScript,
    /// the scope of each `field_values` call: `A` or `R<n>:<sha12 of the sorted keys>`
    pub fv_scopes: Vec<String>,
    /// values of the script whose key the session's model does not have
    pub fv_missing: usize,
    pub pg_calls: usize,
    pub pg_requested: HashSet<u32>,
    pub mh_calls: usize,
}

pub fn scope_desc(keys: &mut [String]) -> String {
    keys.sort();
    let sha = Digest::of(keys.join(",").as_bytes()).short();
    format!("R{}:{}", keys.len(), &sha[..12])
}

impl ReplayOracle {
    pub fn new() -> ReplayOracle {
        ReplayOracle::default()
    }

    /// Starts a step: the script of the TS, a clean record.
    pub fn begin(&mut self, script: StepScript) {
        *self = ReplayOracle { script, ..ReplayOracle::default() };
    }

    pub fn cx<'a>(&'a mut self, clip: &'a mut ClipboardRing) -> ExecCx<'a> {
        ExecCx { layout: self, clipboard: clip }
    }

    /// `fvs` as the capture writes it.
    pub fn fvs(&self) -> String {
        let mut s = self.fv_scopes.len().to_string();
        for d in &self.fv_scopes {
            s.push(';');
            s.push_str(d);
        }
        s
    }
}

impl LayoutOracle for ReplayOracle {
    fn field_values(&mut self, doc: DocView<'_>, scope: &FieldScope) -> Result<Vec<(RunRef, Utf16)>, LayoutError> {
        let n = names(doc);
        let by_key = n.run_of();
        let key_of = n.key_of();
        self.fv_scopes.push(match scope {
            FieldScope::All => "A".to_owned(),
            FieldScope::Runs(set) => {
                if self.script.fv.front().is_some_and(|c| c.unnamed > 0) {
                    format!("R{}:?", set.len())
                } else {
                    let mut keys: Vec<String> = set.iter().filter_map(|r| key_of.get(r).map(|k| (*k).to_owned())).collect();
                    scope_desc(&mut keys)
                }
            }
        });
        let Some(call) = self.script.fv.pop_front() else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        for (k, v) in call.vals {
            match by_key.get(k.as_str()) {
                Some(r) => out.push(((*r).clone(), v)),
                None => self.fv_missing += 1,
            }
        }
        Ok(out)
    }

    fn next_hyphenation(&mut self, doc: DocView<'_>, _from: &DocRef, _skip: &HyphenSkips) -> Result<Option<HyphenCandidate>, LayoutError> {
        self.mh_calls += 1;
        let Some((para, start, end, limit)) = self.script.mh else { return Ok(None) };
        let body = doc.body();
        let bad = || LayoutError { kind: LayoutErrorKind::Internal, detail: "the scripted word is not in the model".into() };
        let p = body.paras.get(para as usize).ok_or_else(bad)?;
        let text = uo_docx::session::text::para_text(p);
        let raw: Vec<u16> = text.get(start..end).ok_or_else(bad)?.to_vec();
        let off = uo_docx::session::ops::references::hyphen::offset_after_letters(&raw, start, f64::from(limit));
        // the block of the paragraph: its index among the blocks
        let mut seen = 0u32;
        let mut block = None;
        for (bi, b) in body.blocks().enumerate() {
            if matches!(b, uo_docx::model::story::Block::Paragraph(_)) {
                if seen == para {
                    block = Some(u32::try_from(bi).unwrap());
                    break;
                }
                seen += 1;
            }
        }
        let block = block.ok_or_else(bad)?;
        Ok(Some(HyphenCandidate {
            at: DocRef { story: StoryRef::Body, path: BlockPath::root(block), offset: u32::try_from(off).unwrap() },
        }))
    }

    fn shown_page_numbers(&mut self, doc: DocView<'_>, paras: &[DocRef]) -> Result<Vec<Option<f64>>, LayoutError> {
        self.pg_calls += 1;
        let n = names(doc);
        let ord = n.ord_of();
        let mut out = Vec::with_capacity(paras.len());
        if std::env::var("UO_F10_TRACE").is_ok() {
            for r in paras {
                if let Some(p) = doc.paragraph(&r.story, &r.path) {
                    println!(
                        "  REQ {} {:?}",
                        r.path,
                        uo_docx::session::text::para_text(p)
                            .iter()
                            .take(24)
                            .map(|&u| char::from_u32(u32::from(u)).unwrap_or('?'))
                            .collect::<String>()
                    );
                }
            }
        }
        for r in paras {
            let o = if r.story == StoryRef::Body { ord.get(&r.path).copied() } else { None };
            if let Some(o) = o {
                self.pg_requested.insert(o);
            }
            out.push(o.and_then(|o| self.script.pg.get(&o).copied()));
        }
        Ok(out)
    }
}
