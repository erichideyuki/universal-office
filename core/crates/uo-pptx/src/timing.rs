//! The timing model (`timing.ts`): the `<p:timing>` tree of a slide and what playback needs from it.
//! The TS tree (`TimeNode.children`, `EffectTiming.node`) is an object graph; here every node lives
//! in a [`TimeArena`] and is referenced by [`NodeId`] (`uo-pptx.md` P5). `parseTimeNode`'s recursion
//! becomes an explicit stack bounded by `Limits::max_time_depth` (frontier B6).

use uo_dml_render::color::CssColor;
use uo_dml_render::ids::Span;

/// A node of the slide's [`TimeArena`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(pub u32);

/// `SlideTransitionInfo` (`timing.ts:32`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TransitionInfo {
    /// The effect element without prefix (`fade`, `push`, `vortex`, `morph`…); `none` without one.
    pub kind: Box<str>,
    /// The element's prefix (`p`, `p14`, `p15`, `p159`).
    pub ns: Box<str>,
    /// The element's attributes in JS `Record` order.
    pub attrs: Vec<(Box<str>, Box<str>)>,
    pub dur_ms: f64,
    pub adv_tm_ms: Option<f64>,
    pub adv_click: bool,
}

/// A chart part target (`<a:chart seriesIdx categoryIdx bldStep>`).
#[derive(Clone, PartialEq, Debug)]
pub struct ChartTarget {
    pub series: f64,
    pub category: f64,
    pub step: Box<str>,
}

/// `AnimTarget` (`timing.ts:48`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct AnimTarget {
    pub spid: Box<str>,
    pub p_range: Option<(f64, f64)>,
    pub char_range: Option<(f64, f64)>,
    pub sub: Option<Box<str>>,
    pub chart: Option<ChartTarget>,
    pub dgm: Option<Box<str>>,
}

/// `TimeCond` (`timing.ts:66`): `delay_ms = None` is "indefinite".
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TimeCond {
    pub delay_ms: Option<f64>,
    pub evt: Option<Box<str>>,
    pub spid: Option<Box<str>>,
    pub bookmark: Option<Box<str>>,
}

/// `<p:iterate>` (`TimeNode.iterate`).
#[derive(Clone, PartialEq, Debug)]
pub struct Iterate {
    /// `el`, `wd` or `lt`.
    pub kind: Box<str>,
    pub backwards: bool,
    pub pct: Option<f64>,
    pub abs_ms: Option<f64>,
}

/// A value of a `<p:tav>` keyframe or a `<p:to>`: a number, a formula/text, or nothing.
#[derive(Clone, PartialEq, Debug)]
pub enum TavValue {
    Num(f64),
    Str(Box<str>),
}

/// `<p:tav>` (`TimeNode.tav`).
#[derive(Clone, PartialEq, Debug)]
pub struct Keyframe {
    pub tm: f64,
    pub val: Option<TavValue>,
    pub fmla: Option<Box<str>>,
    pub color: Option<CssColor>,
}

/// `<p:cMediaNode>` options (`TimeNode.media`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct MediaNodeOpts {
    pub vol: Option<f64>,
    pub mute: Option<bool>,
    pub show_when_stopped: Option<bool>,
    pub num_sld: Option<f64>,
    pub full_scrn: Option<bool>,
}

/// The rare `<p:cTn>` timing attributes of a node (`TimeNode`, `timing.ts:76`): repetition, easing,
/// speed, the time filter, end events and iteration. Boxed: most nodes have none.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct CtnExtras {
    pub repeat: Option<f64>,
    pub auto_rev: Option<bool>,
    pub accel: Option<f64>,
    pub decel: Option<f64>,
    pub spd: Option<f64>,
    pub tm_filter: Option<Vec<(f64, f64)>>,
    pub end_evts: Option<Vec<Box<str>>>,
    pub iterate: Option<Iterate>,
}

impl CtnExtras {
    /// Every field absent.
    pub const EMPTY: CtnExtras =
        CtnExtras { repeat: None, auto_rev: None, accel: None, decel: None, spd: None, tm_filter: None, end_evts: None, iterate: None };
}

/// The kind-specific values of a behaviour or media node (`TimeNode`, `timing.ts:76`): effect
/// properties, scale/motion/rotation/colour values, commands, media options. Boxed inside
/// [`TimeBhvr`].
#[derive(Clone, PartialEq, Debug, Default)]
pub struct BhvrExtras {
    pub pr_lst: Option<Box<str>>,
    pub path: Option<Box<str>>,
    pub by_xy: Option<(f64, f64)>,
    pub from_xy: Option<(f64, f64)>,
    pub to_xy: Option<(f64, f64)>,
    pub rot_by: Option<f64>,
    pub rot_from: Option<f64>,
    pub rot_to: Option<f64>,
    pub from_color: Option<CssColor>,
    pub by_hsl: Option<(f64, f64, f64)>,
    pub clr_spc: Option<Box<str>>,
    pub clr_dir: Option<Box<str>>,
    pub r_ang: Option<f64>,
    pub origin: Option<Box<str>>,
    pub path_edit_mode: Option<Box<str>>,
    pub cmd: Option<Box<str>>,
    pub cmd_type: Option<Box<str>>,
    pub media: Option<MediaNodeOpts>,
}

impl BhvrExtras {
    /// Every field absent.
    pub const EMPTY: BhvrExtras = BhvrExtras {
        pr_lst: None,
        path: None,
        by_xy: None,
        from_xy: None,
        to_xy: None,
        rot_by: None,
        rot_from: None,
        rot_to: None,
        from_color: None,
        by_hsl: None,
        clr_spc: None,
        clr_dir: None,
        r_ang: None,
        origin: None,
        path_edit_mode: None,
        cmd: None,
        cmd_type: None,
        media: None,
    };
}

/// The values of a behaviour (`set`, `anim`, `animEffect`, …) or media node (`TimeNode`,
/// `timing.ts:76`): the target and the common behaviour attributes; the kind-specific rest is the
/// boxed [`BhvrExtras`]. Containers (`par`, `seq`, `excl`) have none.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TimeBhvr {
    pub target: Option<AnimTarget>,
    pub attr_names: Option<Vec<Box<str>>>,
    pub additive: Option<Box<str>>,
    pub filter: Option<Box<str>>,
    pub transition: Option<Box<str>>,
    pub set_to: Option<Box<str>>,
    pub to_color: Option<CssColor>,
    pub calcmode: Option<Box<str>>,
    pub value_type: Option<Box<str>>,
    pub from: Option<Box<str>>,
    pub to: Option<Box<str>>,
    pub by: Option<Box<str>>,
    pub tav: Option<Vec<Keyframe>>,
    pub extras: Option<Box<BhvrExtras>>,
}

impl TimeBhvr {
    /// Every field absent.
    pub const EMPTY: TimeBhvr = TimeBhvr {
        target: None,
        attr_names: None,
        additive: None,
        filter: None,
        transition: None,
        set_to: None,
        to_color: None,
        calcmode: None,
        value_type: None,
        from: None,
        to: None,
        by: None,
        tav: None,
        extras: None,
    };
}

static EMPTY_CTN_EXTRAS: CtnExtras = CtnExtras::EMPTY;
static EMPTY_BHVR: TimeBhvr = TimeBhvr::EMPTY;
static EMPTY_BHVR_EXTRAS: BhvrExtras = BhvrExtras::EMPTY;

/// A container (`par`/`seq`/`excl`) or behaviour (`set`, `anim`, `animEffect`, `animMotion`,
/// `animRot`, `animScale`, `animClr`, `cmd`, `audio`, `video`) of the time tree (`timing.ts:76`).
/// The `<p:cTn>` attributes every node has are inline; the rare ones ([`TimeNode::ctn_x`]), the
/// behaviour values ([`TimeNode::bhvr`]) and their kind-specific part ([`TimeNode::bhvr_x`]) are boxed
/// groups, read through accessors that answer "absent" when a box is (memory compaction, `uo-pptx.md`
/// "Model memory").
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TimeNode {
    pub tag: Box<str>,
    pub id: Option<Box<str>>,
    pub st: Vec<TimeCond>,
    /// ms; `f64::INFINITY` = indefinite.
    pub dur: Option<f64>,
    pub fill: Option<Box<str>>,
    pub node_type: Option<Box<str>>,
    pub preset_class: Option<Box<str>>,
    pub preset_id: Option<f64>,
    pub preset_subtype: Option<f64>,
    pub grp_id: Option<Box<str>>,
    pub ctn_extras: Option<Box<CtnExtras>>,
    pub behavior: Option<Box<TimeBhvr>>,
    /// The children in [`TimeArena::children`].
    pub children: Span,
}

impl TimeNode {
    /// The rare `<p:cTn>` attributes (all absent when the box is).
    pub fn ctn_x(&self) -> &CtnExtras {
        self.ctn_extras.as_deref().unwrap_or(&EMPTY_CTN_EXTRAS)
    }

    /// The rare `<p:cTn>` attributes, created empty on first write.
    pub fn ctn_x_mut(&mut self) -> &mut CtnExtras {
        self.ctn_extras.get_or_insert_with(Default::default)
    }

    /// The behaviour values (all absent when the box is).
    pub fn bhvr(&self) -> &TimeBhvr {
        self.behavior.as_deref().unwrap_or(&EMPTY_BHVR)
    }

    /// The behaviour values, created empty on first write.
    pub fn bhvr_mut(&mut self) -> &mut TimeBhvr {
        self.behavior.get_or_insert_with(Default::default)
    }

    /// The kind-specific behaviour values (all absent when either box is).
    pub fn bhvr_x(&self) -> &BhvrExtras {
        self.bhvr().extras.as_deref().unwrap_or(&EMPTY_BHVR_EXTRAS)
    }

    /// The kind-specific behaviour values, created empty on first write.
    pub fn bhvr_x_mut(&mut self) -> &mut BhvrExtras {
        self.bhvr_mut().extras.get_or_insert_with(Default::default)
    }
}

/// Every node of a slide's time tree; a node's `children` is a span of `children`.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TimeArena {
    pub nodes: Vec<TimeNode>,
    pub children: Vec<NodeId>,
}

/// An effect placed in its step (`EffectTiming`, `timing.ts:153`).
#[derive(Clone, PartialEq, Debug)]
pub struct EffectTiming {
    pub id: Option<Box<str>>,
    pub preset_class: Box<str>,
    pub preset_id: f64,
    pub preset_subtype: f64,
    pub node_type: Option<Box<str>>,
    pub target: Option<AnimTarget>,
    pub start_ms: f64,
    pub dur_ms: f64,
    pub block_ms: f64,
    pub fill: Option<Box<str>>,
    pub filter: Option<Box<str>>,
    pub motion: Option<bool>,
    pub end_on_next: Option<bool>,
    pub node: NodeId,
}

/// A step of the main sequence (`AnimStep`, `timing.ts:184`).
#[derive(Clone, PartialEq, Debug)]
pub struct AnimStep {
    pub auto: bool,
    pub dur_ms: f64,
    pub has_indefinite: bool,
    pub effects: Vec<EffectTiming>,
}

/// `BookmarkTrigger` (`timing.ts:215`).
#[derive(Clone, PartialEq, Debug)]
pub struct BookmarkTrigger {
    pub spid: Box<str>,
    pub bookmark: Box<str>,
    pub time_ms: Option<f64>,
    pub trim_start_ms: Option<f64>,
    pub effects: Vec<EffectTiming>,
}

/// `SlideMediaInfo` (`timing.ts:226`).
#[derive(Clone, PartialEq, Debug)]
pub struct SlideMediaInfo {
    pub spid: Box<str>,
    pub kind: uo_dml_render::model::MediaKind,
    pub vol: Option<f64>,
    pub mute: Option<bool>,
    pub show_when_stopped: Option<bool>,
    pub rewind: Option<bool>,
    pub loop_: Option<bool>,
    pub full_screen: Option<bool>,
}

/// `SlideTimingModel` (`timing.ts:194`) with its node arena.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TimingModel {
    pub transition: Option<TransitionInfo>,
    pub steps: Vec<AnimStep>,
    pub interactive: Vec<EffectTiming>,
    pub hidden: bool,
    pub anim_bg: Option<Vec<Box<str>>>,
    pub media: Option<Vec<SlideMediaInfo>>,
    pub bookmark_triggers: Option<Vec<BookmarkTrigger>>,
    pub nodes: TimeArena,
}

impl TimingModel {
    /// Releases the spare capacity of every vector of the model (the node arena above all: a `Vec` of
    /// nodes grown by pushes keeps up to twice the nodes). Values are not touched.
    pub fn shrink_to_fit(&mut self) {
        if let Some(t) = self.transition.as_mut() {
            t.attrs.shrink_to_fit();
        }
        self.steps.shrink_to_fit();
        for s in &mut self.steps {
            s.effects.shrink_to_fit();
        }
        self.interactive.shrink_to_fit();
        for v in self.anim_bg.iter_mut() {
            v.shrink_to_fit();
        }
        for v in self.media.iter_mut() {
            v.shrink_to_fit();
        }
        for v in self.bookmark_triggers.iter_mut() {
            v.shrink_to_fit();
            for b in v {
                b.effects.shrink_to_fit();
            }
        }
        self.nodes.nodes.shrink_to_fit();
        self.nodes.children.shrink_to_fit();
        for n in &mut self.nodes.nodes {
            n.st.shrink_to_fit();
            if let Some(x) = n.ctn_extras.as_deref_mut() {
                for v in x.tm_filter.iter_mut() {
                    v.shrink_to_fit();
                }
                for v in x.end_evts.iter_mut() {
                    v.shrink_to_fit();
                }
            }
            if let Some(b) = n.behavior.as_deref_mut() {
                for v in b.attr_names.iter_mut() {
                    v.shrink_to_fit();
                }
                for v in b.tav.iter_mut() {
                    v.shrink_to_fit();
                }
            }
        }
    }
}
