//! Duration evaluation of the time tree (`timing.ts:533–733`): `beginOffset`, `simpleDur`, `blockDur`,
//! `activeDur`, the collection of the effects of a step (`collectEffects`, `collectGroup`, `stepFrom`).
//! The TS recursion over `TimeNode.children` runs here over the [`TimeArena`]; the depth is bounded by
//! the parser (`Limits::max_time_depth`), so the recursion is bounded too.

use uo_jsnum::math;

use crate::timing::{AnimStep, AnimTarget, EffectTiming, NodeId, TimeArena, TimeNode};

/// `STEP_PENDING` (`timing.ts:549`): a step not yet fired starts "far enough to never arrive".
pub const STEP_PENDING: f64 = 1e12;

/// `AFTER_LINK_MS` (`timing.ts:698`): the delay of each "after previous" link.
pub const AFTER_LINK_MS: f64 = 1.0;

/// The context of the duration evaluation (`TimingEvalCtx`): the text unit count and the animated
/// background test.
pub struct EvalCtx<'a> {
    pub units: &'a dyn Fn(Option<&AnimTarget>, &str) -> f64,
    pub anim_bg: &'a dyn Fn(&str, Option<&str>) -> bool,
}

impl TimeArena {
    /// The node (`NodeId`s of a model always resolve; a foreign id gives the empty node).
    pub fn node(&self, id: NodeId) -> &TimeNode {
        static EMPTY: std::sync::OnceLock<TimeNode> = std::sync::OnceLock::new();
        self.nodes.get(id.0 as usize).unwrap_or_else(|| EMPTY.get_or_init(TimeNode::default))
    }

    /// The children of a node, in order.
    pub fn kids(&self, n: &TimeNode) -> &[NodeId] {
        let s = n.children.start as usize;
        let e = s.saturating_add(n.children.len as usize);
        self.children.get(s..e).unwrap_or(&[])
    }
}

/// `beginOffset` (`timing.ts:534`): offset of a child inside a `par` (ms); `None` = by event/click only.
pub fn begin_offset(n: &TimeNode) -> Option<f64> {
    if n.st.is_empty() {
        return Some(0.0);
    }
    for c in &n.st {
        let Some(d) = c.delay_ms else { continue };
        if c.evt.as_deref().is_none_or(|e| e.is_empty() || e == "onBegin") {
            return Some(d);
        }
    }
    None
}

/// The target is the whole shape (no `<p:txEl>`/`<p:bg>`/sub-target).
pub fn whole_shape(t: &AnimTarget) -> bool {
    t.p_range.is_none() && t.char_range.is_none() && t.sub.is_none()
}

/// `firstTarget` (`timing.ts:556`): the node's own target, else the first of its descendants (pre-order).
pub fn first_target(a: &TimeArena, id: NodeId) -> Option<&AnimTarget> {
    let n = a.node(id);
    if let Some(t) = &n.target {
        return Some(t);
    }
    a.kids(n).iter().find_map(|c| first_target(a, *c))
}

/// Simple duration of the node (one iteration, no repetition).
pub fn simple_dur(a: &TimeArena, id: NodeId, ctx: &EvalCtx<'_>) -> f64 {
    let n = a.node(id);
    let mut base;
    if let Some(d) = n.dur {
        base = d;
    } else if &*n.tag == "seq" {
        base = 0.0;
        for c in a.kids(n) {
            let Some(b) = begin_offset(a.node(*c)) else { break };
            base += b + active_dur(a, *c, ctx);
        }
    } else {
        base = 0.0;
        for c in a.kids(n) {
            let Some(b) = begin_offset(a.node(*c)) else { continue };
            base = math::max(base, b + active_dur(a, *c, ctx));
        }
    }
    if let Some(it) = &n.iterate
        && base.is_finite()
    {
        let tgt = first_target(a, id);
        let count = math::max(1.0, (ctx.units)(tgt, &it.kind));
        let gap = it.abs_ms.unwrap_or_else(|| it.pct.unwrap_or(0.0) * base);
        // the shape's background: 1st unit of full duration, before the text (only with the whole shape
        // as the target — a range of paragraphs/characters does not take the background)
        let bg = match tgt {
            Some(t) if whole_shape(t) && (ctx.anim_bg)(&t.spid, n.grp_id.as_deref()) => base,
            _ => 0.0,
        };
        base += bg + (count - 1.0) * gap;
    }
    base
}

/// The duration that blocks the sequence: as [`active_dur`], but indefinite stretches count 0 (the spread
/// of `<p:iterate>` still counts).
pub fn block_dur(a: &TimeArena, id: NodeId, ctx: &EvalCtx<'_>) -> f64 {
    let d = active_dur(a, id, ctx);
    if d.is_finite() {
        return d;
    }
    let n = a.node(id);
    if n.repeat == Some(f64::INFINITY) {
        return 0.0;
    }
    let mut base = 0.0;
    for c in a.kids(n) {
        let Some(b) = begin_offset(a.node(*c)) else { continue };
        base = math::max(base, b + block_dur(a, *c, ctx));
    }
    if let Some(it) = &n.iterate {
        let tgt = first_target(a, id);
        let count = math::max(1.0, (ctx.units)(tgt, &it.kind));
        let gap = it.abs_ms.unwrap_or(0.0);
        base += (count - 1.0) * gap;
    }
    base
}

/// Active duration: simple × (2 with `autoRev`) × repetitions. `spd` does NOT enter (measured in the oracle).
pub fn active_dur(a: &TimeArena, id: NodeId, ctx: &EvalCtx<'_>) -> f64 {
    let n = a.node(id);
    let mut d = simple_dur(a, id, ctx) * if n.auto_rev == Some(true) { 2.0 } else { 1.0 };
    if let Some(r) = n.repeat {
        d = if r == f64::INFINITY { f64::INFINITY } else { d * r };
    }
    d
}

fn first_of<'a>(a: &'a TimeArena, id: NodeId, tag: &str) -> Option<&'a TimeNode> {
    let n = a.node(id);
    if &*n.tag == tag {
        return Some(n);
    }
    a.kids(n).iter().find_map(|c| first_of(a, *c, tag))
}

/// The effects (nodes with `presetClass`) under `n`, with the start relative to `t0`.
fn collect_effects(a: &TimeArena, id: NodeId, t0: f64, ctx: &EvalCtx<'_>, out: &mut Vec<EffectTiming>) {
    let n = a.node(id);
    if n.preset_class.as_deref().is_some_and(|p| !p.is_empty()) {
        let mut fx = EffectTiming {
            id: None,
            preset_class: n.preset_class.clone().unwrap_or_default(),
            preset_id: n.preset_id.unwrap_or(0.0),
            preset_subtype: n.preset_subtype.unwrap_or(0.0),
            node_type: None,
            target: None,
            start_ms: t0,
            dur_ms: active_dur(a, id, ctx),
            block_ms: block_dur(a, id, ctx),
            fill: None,
            filter: None,
            motion: None,
            end_on_next: None,
            node: id,
        };
        fx.id = n.id.clone().filter(|s| !s.is_empty());
        fx.node_type = n.node_type.clone().filter(|s| !s.is_empty());
        fx.fill = n.fill.clone().filter(|s| !s.is_empty());
        fx.target = first_target(a, id).cloned();
        if let Some(ae) = first_of(a, id, "animEffect")
            && let Some(f) = ae.filter.as_ref().filter(|s| !s.is_empty())
        {
            fx.filter = Some(f.clone());
        }
        if first_of(a, id, "animMotion").is_some() {
            fx.motion = Some(true);
        }
        if n.end_evts.as_ref().is_some_and(|e| e.iter().any(|x| &**x == "onNext")) {
            fx.end_on_next = Some(true);
        }
        out.push(fx);
        return;
    }
    if &*n.tag == "seq" {
        let mut t = t0;
        for c in a.kids(n) {
            let Some(b) = begin_offset(a.node(*c)) else { break };
            t += b;
            collect_effects(a, *c, t, ctx, out);
            t += active_dur(a, *c, ctx);
        }
        return;
    }
    for c in a.kids(n) {
        let Some(b) = begin_offset(a.node(*c)) else { continue };
        collect_effects(a, *c, t0 + b, ctx, out);
    }
}

/// `nodeType` of the first effect under `n` (clickEffect/withEffect/afterEffect).
fn first_node_type(a: &TimeArena, id: NodeId) -> Option<&str> {
    let n = a.node(id);
    if n.preset_class.as_deref().is_some_and(|p| !p.is_empty()) {
        return n.node_type.as_deref().filter(|s| !s.is_empty());
    }
    a.kids(n).iter().find_map(|c| first_node_type(a, *c))
}

/// The effects of a click group: "after previous" sub-groups start when the previous sub-group ENDS
/// (dynamic chaining), not at the recorded `delay`; the others use their own `delay`. Returns the end.
/// `group` is the node whose children are the sub-groups (the TS passes a copy of the node with `st`
/// emptied: only the children are read).
pub fn collect_group(a: &TimeArena, group: NodeId, t0: f64, ctx: &EvalCtx<'_>, out: &mut Vec<EffectTiming>) -> f64 {
    let mut prev_end = t0;
    let mut end = t0;
    for (i, sub) in a.kids(a.node(group)).iter().enumerate() {
        let Some(b) = begin_offset(a.node(*sub)) else { continue };
        let start = if i > 0 && first_node_type(a, *sub) == Some("afterEffect") { prev_end + AFTER_LINK_MS } else { t0 + b };
        collect_effects(a, *sub, start, ctx, out);
        prev_end = start + block_dur(a, *sub, ctx);
        end = math::max(end, prev_end);
    }
    end
}

/// `stepFrom` (`timing.ts:719`): a step from `(node, offset)` pairs.
pub fn step_from(a: &TimeArena, nodes: &[(NodeId, f64)], auto: bool, ctx: &EvalCtx<'_>) -> AnimStep {
    let mut effects = Vec::new();
    let mut t = 0.0;
    for (id, offset) in nodes {
        t += offset;
        let n = a.node(*id);
        t = if n.preset_class.as_deref().is_some_and(|p| !p.is_empty()) {
            collect_effects(a, *id, t, ctx, &mut effects);
            t + active_dur(a, *id, ctx)
        } else {
            collect_group(a, *id, t, ctx, &mut effects)
        };
    }
    let mut dur = 0.0;
    let mut has_indefinite = false;
    for e in &effects {
        dur = math::max(dur, e.start_ms + e.block_ms);
        if !e.dur_ms.is_finite() {
            has_indefinite = true;
        }
    }
    AnimStep { auto, dur_ms: dur, has_indefinite, effects }
}
