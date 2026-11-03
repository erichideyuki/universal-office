//! The ligature index of `ligate` (stage 5, uo-fonts D5): per lookup and per first glyph whose
//! list holds more than [`SCAN_MAX`] ligatures, a radix trie of the component sequences
//! (`Ligature::rest`) that keeps, at each node where ligatures end, the lowest index in the first
//! glyph's list (the font order). Shorter lists (80% of the 2,746 lists of 256 local fonts) are
//! scanned in font order as the TS does: at most `SCAN_MAX` candidates per position, and no index
//! memory for them.
//!
//! Why the result is the TS one: `applyLigatures` takes the first ligature of the first glyph's
//! list whose `rest` equals the groups that follow. A ligature matches exactly when its `rest` is
//! a prefix of the following `gid`s, and the prefixes of that sequence are the nodes met while
//! walking it from the root (ligatures end only at nodes: an edge holds no end). So the first
//! match is the lowest index among the ends of the walked nodes, and each node keeps the lowest
//! index of the ligatures that end there (ligatures are inserted in list order and the first one
//! to end at a node stays). The cost of a position is its depth (one child search and the
//! components of each edge), not the length of the list.
//!
//! Edges are compressed (an edge spells several components): its components are read from the
//! `rest` of a ligature that passes through it (`rep`), so a long `rest` costs one node, not one
//! node per component. Nodes are 16 bytes, children contiguous and sorted by their first component.
//! Building is linear in the components of the list (each ligature walks its `rest` once; an
//! entry equal to the one before it is skipped), work the parse has already counted. Lists equal
//! in content share one trie, and the nodes are capped (one per 16 entries of the parse's store
//! cap): a list past the cap is scanned, as before D5, under the query budget.

use std::hash::{Hash, Hasher};

use rustc_hash::{FxHashMap, FxHasher};

use crate::error::{FontError, LimitKind};
use crate::gsub::{LigGroup, Ligature};
use crate::limits::WorkBudget;

const ENDS: u32 = 1 << 31;

/// The longest list that is scanned instead of indexed.
pub(crate) const SCAN_MAX: usize = 8;

/// One node: the edge from its parent spells `rest[parent.to..to]` of the ligature `rep` of the
/// first glyph's list (its first component is `key`); when a ligature ends here, `rep` is the
/// lowest-indexed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Node {
    first_child: u32,
    /// the number of children; bit 31 set when a ligature ends here
    children: u32,
    rep: u32,
    to: u16,
    key: u16,
}

impl Node {
    fn ends(self) -> bool {
        self.children & ENDS != 0
    }

    fn child_count(self) -> u32 {
        self.children & !ENDS
    }
}

/// The index of a GSUB: the root of each (lookup, first glyph) whose list is longer than
/// [`SCAN_MAX`] (keyed `lookup << 16 | first`) and the nodes of every trie. Lists equal in content
/// share one trie (a coverage that points many glyphs, or many lookups, at the same ligature sets
/// makes equal lists: the index then grows with the distinct lists, not with the copies the parse
/// stores). The maps are `u32 -> u32` and `u64 -> u32` so that the wasm module reuses their code.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct LigIndex {
    roots: FxHashMap<u32, u32>,
    nodes: Box<[Node]>,
}

fn index_u32(n: usize) -> Result<u32, FontError> {
    u32::try_from(n).ok().filter(|&v| v < ENDS).ok_or(FontError::Limit(LimitKind::KernPairs))
}

fn root_key(lookup: u16, first: u16) -> u32 {
    (u32::from(lookup) << 16) | u32::from(first)
}

/// A hash of a list's content (results and components, in order), to find equal lists.
fn content_hash(list: &[Ligature]) -> u64 {
    let mut h = FxHasher::default();
    list.len().hash(&mut h);
    for lig in list {
        lig.lig.hash(&mut h);
        lig.rest.hash(&mut h);
    }
    h.finish()
}

impl LigIndex {
    /// Indexes the lists longer than [`SCAN_MAX`] of every lookup (lookup index to first glyph to
    /// its list in font order) as long as the nodes stay within `max_nodes`: a list whose trie
    /// would pass it is left to the scan (the stage-4 search, the same results, its work under
    /// the query budget), so the index never holds more than `16 x max_nodes` bytes of nodes.
    pub(crate) fn build(lookups: &FxHashMap<u16, FxHashMap<u16, Vec<Ligature>>>, max_nodes: usize) -> Result<LigIndex, FontError> {
        let mut roots: FxHashMap<u32, u32> = FxHashMap::default();
        let mut nodes: Vec<Node> = Vec::new();
        // the first list seen with each content hash (its root key); a list equal to it reuses its
        // root (one comparison per list: colliding unequal lists are indexed on their own)
        let mut seen: FxHashMap<u64, u32> = FxHashMap::default();
        let list_of = |key: u32| lookups.get(&((key >> 16) as u16)).and_then(|m| m.get(&(key as u16)));
        let mut builder = Builder::default();
        for (&li, map) in lookups {
            for (&first, list) in map {
                if list.len() <= SCAN_MAX {
                    continue;
                }
                let key = root_key(li, first);
                let hash = content_hash(list);
                if let Some(&other) = seen.get(&hash)
                    && list_of(other) == Some(list)
                    && let Some(&root) = roots.get(&other)
                {
                    roots.insert(key, root);
                    continue;
                }
                let room = max_nodes.saturating_sub(nodes.len());
                builder.clear();
                let mut previous: Option<&Ligature> = None;
                for (i, lig) in list.iter().enumerate() {
                    if builder.nodes.len() > room {
                        break;
                    }
                    // an entry equal in `rest` to the one before it can never be the first match
                    if previous.is_some_and(|p| p.rest == lig.rest) {
                        continue;
                    }
                    previous = Some(lig);
                    builder.insert(list, index_u32(i)?)?;
                }
                if builder.nodes.len() > room {
                    continue;
                }
                let root = index_u32(nodes.len())?;
                builder.freeze_into(&mut nodes)?;
                roots.insert(key, root);
                seen.entry(hash).or_insert(key);
            }
        }
        roots.shrink_to_fit();
        Ok(LigIndex { roots, nodes: nodes.into_boxed_slice() })
    }

    /// The root of the trie of a (lookup, first glyph) list, if it has one.
    fn root(&self, lookup: u16, first: u16) -> Option<Node> {
        self.roots.get(&root_key(lookup, first)).and_then(|&r| self.nodes.get(r as usize)).copied()
    }

    /// The trie walk of [`first_match`] from `root`.
    fn walk<'a>(
        &self,
        root: Node,
        list: &'a [Ligature],
        next: &[LigGroup],
        budget: &mut WorkBudget,
    ) -> Result<Option<&'a Ligature>, FontError> {
        let mut node = root;
        let mut best: Option<u32> = None;
        loop {
            if node.ends() {
                best = Some(best.map_or(node.rep, |b| b.min(node.rep)));
            }
            let depth = usize::from(node.to);
            let Some(gid) = next.get(depth).and_then(|g| u16::try_from(g.gid).ok()) else { break };
            let count = node.child_count();
            if count == 0 {
                break;
            }
            budget.tick_n(u64::from(u32::BITS - count.leading_zeros()))?;
            let start = node.first_child as usize;
            let Some(children) = self.nodes.get(start..start + count as usize) else { break };
            let Ok(at) = children.binary_search_by_key(&gid, |c| c.key) else { break };
            let Some(&child) = children.get(at) else { break };
            let Some(label) = list.get(child.rep as usize).map(|l| &l.rest) else { break };
            let mut whole = true;
            for j in depth + 1..usize::from(child.to) {
                budget.tick()?;
                if next.get(j).map(|g| g.gid) != label.get(j).map(|&r| u32::from(r)) {
                    whole = false;
                    break;
                }
            }
            if !whole {
                break;
            }
            node = child;
        }
        Ok(best.and_then(|b| list.get(b as usize)))
    }

    /// Bytes held by the index (nodes, and the root table as hashbrown sizes it: 8 bytes and a
    /// control byte per bucket; M3).
    pub(crate) fn heap_bytes(&self) -> usize {
        let buckets = if self.roots.is_empty() { 0 } else { (self.roots.capacity() * 8 / 7).next_power_of_two() };
        size_of_val(&*self.nodes) + buckets * (size_of::<(u32, u32)>() + 1)
    }
}

/// The first ligature, in font order, of the `list` of `first` in `lookup` whose `rest` equals
/// the `gid`s of the groups in `next` (`None` when none does). A list with a trie (longer than
/// [`SCAN_MAX`], within the node cap) is searched in it: one unit of `budget` per child search
/// probe (the bit length of the number of children) and per further component compared on an
/// edge. Any other list is scanned as the TS does: one unit per candidate examined and per
/// component compared.
pub(crate) fn first_match<'a>(
    index: &LigIndex,
    lookup: u16,
    first: u16,
    list: &'a [Ligature],
    next: &[LigGroup],
    budget: &mut WorkBudget,
) -> Result<Option<&'a Ligature>, FontError> {
    if list.len() > SCAN_MAX
        && let Some(root) = index.root(lookup, first)
    {
        return index.walk(root, list, next, budget);
    }
    for candidate in list {
        budget.tick()?;
        let mut matches = true;
        for (k, &r) in candidate.rest.iter().enumerate() {
            budget.tick()?;
            if next.get(k).map(|g| g.gid) != Some(u32::from(r)) {
                matches = false;
                break;
            }
        }
        if matches {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn edge(parent: u32, key: u16) -> u64 {
    (u64::from(parent) << 16) | u64::from(key)
}

/// A node while building: its edge, whether a ligature ends here.
#[derive(Debug, Clone, Copy)]
struct Draft {
    rep: u32,
    to: u16,
    ends: bool,
}

/// The trie of one first glyph while it is built: nodes (0 is the root) and edges by (parent,
/// first component); reused from one first glyph to the next.
#[derive(Debug, Default)]
struct Builder {
    nodes: Vec<Draft>,
    /// `parent << 16 | first component` to the child
    edges: FxHashMap<u64, u32>,
}

impl Builder {
    fn clear(&mut self) {
        self.nodes.clear();
        self.edges.clear();
        self.nodes.push(Draft { rep: 0, to: 0, ends: false });
    }

    fn push(&mut self, draft: Draft) -> Result<u32, FontError> {
        let at = index_u32(self.nodes.len())?;
        self.nodes.push(draft);
        Ok(at)
    }

    /// Adds the ligature `i` of `list` (inserted in increasing `i`, so the first end at a node is
    /// the lowest index).
    fn insert(&mut self, list: &[Ligature], i: u32) -> Result<(), FontError> {
        let Some(rest) = list.get(i as usize).map(|l| &l.rest) else { return Ok(()) };
        // a `rest` has at most 65,534 components (a u16 count, minus the first glyph)
        let len = u16::try_from(rest.len()).map_err(|_| FontError::Limit(LimitKind::KernPairs))?;
        let mut current = 0u32;
        let mut depth = 0usize;
        loop {
            let Some(&key) = rest.get(depth) else {
                if let Some(node) = self.nodes.get_mut(current as usize)
                    && !node.ends
                {
                    *node = Draft { rep: i, to: node.to, ends: true };
                }
                return Ok(());
            };
            let Some(&child) = self.edges.get(&edge(current, key)) else {
                let leaf = self.push(Draft { rep: i, to: len, ends: true })?;
                self.edges.insert(edge(current, key), leaf);
                return Ok(());
            };
            let Some(&c) = self.nodes.get(child as usize) else { return Ok(()) };
            let Some(label) = list.get(c.rep as usize).map(|l| &l.rest) else { return Ok(()) };
            let to = usize::from(c.to);
            let mut j = depth + 1;
            while j < to && rest.get(j).is_some_and(|r| label.get(j) == Some(r)) {
                j += 1;
            }
            if j == to {
                current = child;
                depth = to;
                continue;
            }
            // the ligature leaves the edge at component j: split the edge there
            let split = u16::try_from(j).map_err(|_| FontError::Limit(LimitKind::KernPairs))?;
            let Some(&label_j) = label.get(j) else { return Ok(()) };
            let ends_here = j == rest.len();
            let mid = self.push(Draft { rep: if ends_here { i } else { c.rep }, to: split, ends: ends_here })?;
            self.edges.insert(edge(current, key), mid);
            self.edges.insert(edge(mid, label_j), child);
            if let Some(&r) = rest.get(j) {
                let leaf = self.push(Draft { rep: i, to: len, ends: true })?;
                self.edges.insert(edge(mid, r), leaf);
            }
            return Ok(());
        }
    }

    /// Appends the trie to `out` breadth first, the root first, each node's children contiguous
    /// and sorted by their first component.
    fn freeze_into(&self, out: &mut Vec<Node>) -> Result<(), FontError> {
        let mut edges: Vec<u64> = self.edges.keys().copied().collect();
        edges.sort_unstable();
        let mut starts = vec![0u32; self.nodes.len() + 1];
        for parent in edges.iter().map(|&e| e >> 16) {
            if let Some(s) = starts.get_mut(parent as usize + 1) {
                *s += 1;
            }
        }
        for k in 1..starts.len() {
            let before = starts.get(k - 1).copied().unwrap_or(0);
            if let Some(s) = starts.get_mut(k) {
                *s += before;
            }
        }
        let base = out.len();
        let node_of = |draft: usize, key: u16| -> Node {
            let d = self.nodes.get(draft).copied().unwrap_or(Draft { rep: 0, to: 0, ends: false });
            Node { first_child: 0, children: if d.ends { ENDS } else { 0 }, rep: d.rep, to: d.to, key }
        };
        let mut drafts: Vec<u32> = Vec::with_capacity(self.nodes.len());
        out.push(node_of(0, 0));
        drafts.push(0);
        let mut k = 0usize;
        while let Some(&draft) = drafts.get(k) {
            let from = starts.get(draft as usize).copied().unwrap_or(0) as usize;
            let to = starts.get(draft as usize + 1).copied().unwrap_or(0) as usize;
            let kids = edges.get(from..to).unwrap_or(&[]);
            let first_child = index_u32(out.len())?;
            if let Some(node) = out.get_mut(base + k) {
                node.first_child = first_child;
                node.children |= index_u32(kids.len())?;
            }
            for &e in kids {
                let child = self.edges.get(&e).copied().unwrap_or(0);
                out.push(node_of(child as usize, e as u16));
                drafts.push(child);
            }
            k += 1;
        }
        Ok(())
    }
}
