//! Media in playback (`media.ts`): video/audio play through COMMANDS of the time tree (`<p:cmd type="call"
//! cmd="playFrom(s)">`, `pause`, `resume`, `togglePause`, `stop`: effects of `presetClass="mediacall"`) and the
//! animated GIF by its own frame delays. Pure functions: whoever owns the media elements (the shell) decides
//! how to seek (real time or exact seek for deterministic capture).
//!
//! Measured in the oracle (video export): the shape shows the COVER until the command; the media plays in real
//! time; at the end it FREEZES on the last frame. The export video starts 2 TICKS after the command and
//! advances 1/30 s per exported frame; the media node ends at command + duration (on that clock).

use uo_jsnum::{math, parse, text};

use crate::playback::PlacedEffect;
use crate::timing::{NodeId, TimeArena, TimingModel};
use crate::timing_eval::{STEP_PENDING, begin_offset};

/// The media clock: start latency and scale (video export).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct MediaClock {
    /// Delay between the command and the 1st frame (timeline ms). Default 0.
    pub latency_ms: Option<f64>,
    /// Media ms per timeline ms. Default 1.
    pub scale: Option<f64>,
}

/// A media command at an absolute animation instant of the slide.
#[derive(Clone, PartialEq, Debug)]
pub struct MediaCommand {
    pub at_ms: f64,
    /// `playFrom(0.0)`, `pause`, `resume`, `togglePause`, `stop`…
    pub cmd: Box<str>,
    /// The command's `dur` (the application records the media duration on `playFrom`), ms.
    pub dur_ms: Option<f64>,
}

/// The playback state of a media at an instant (`MediaPlayState`).
#[derive(Clone, PartialEq, Debug)]
pub struct MediaPlayState {
    /// Some `playFrom` already fired (before: the cover frame).
    pub started: bool,
    pub playing: bool,
    /// Wanted position (s), not limited by the media's duration.
    pub time_sec: f64,
    /// The media's duration per the file (ms), when known.
    pub dur_ms: Option<f64>,
    /// The media node ENDED (≠ paused): "rewind" / "hide when stopped" apply.
    pub ended: Option<bool>,
}

/// The commands (`<p:cmd>`) of the placed effects that aim at the shape `spid`, in order.
pub fn media_commands(arena: &TimeArena, effects: &[PlacedEffect], spid: &str) -> Vec<MediaCommand> {
    fn walk(a: &TimeArena, id: NodeId, t: f64, spid: &str, out: &mut Vec<MediaCommand>) {
        let n = a.node(id);
        if &*n.tag == "cmd" {
            if let Some(cmd) = n.bhvr_x().cmd.as_deref().filter(|c| !c.is_empty())
                && n.bhvr().target.as_ref().is_some_and(|t| &*t.spid == spid)
            {
                out.push(MediaCommand { at_ms: t, cmd: cmd.into(), dur_ms: n.dur.filter(|d| d.is_finite()) });
            }
            return;
        }
        for c in a.kids(n) {
            if let Some(b) = begin_offset(a.node(*c)) {
                walk(a, *c, t + b, spid, out);
            }
        }
    }
    let mut out = Vec::new();
    // pending (step not given, interactive trigger) do not play — not even at the end of the slide (∞)
    for e in effects {
        if e.t0 < STEP_PENDING {
            walk(arena, e.fx.node, e.t0, spid, &mut out);
        }
    }
    uo_jsnum::sort::js_sort_by(&mut out, |a, b| a.at_ms - b.at_ms);
    out
}

/// `/^playFrom\(\s*(-?[\d.]+)\s*\)$/`: the captured number text.
fn play_from_arg(cmd: &str) -> Option<&str> {
    let rest = cmd.strip_prefix("playFrom(")?;
    let rest = rest.trim_start_matches(text::is_js_space);
    let n = number_len(rest);
    if n == 0 {
        return None;
    }
    let (num, tail) = rest.split_at(n);
    let tail = tail.trim_start_matches(text::is_js_space);
    (tail == ")").then_some(num)
}

/// Length of `-?[\d.]+` at the start of `s`.
fn number_len(s: &str) -> usize {
    let b = s.as_bytes();
    let neg = usize::from(b.first() == Some(&b'-'));
    let n = b[neg..].iter().take_while(|c| c.is_ascii_digit() || **c == b'.').count();
    if n == 0 { 0 } else { neg + n }
}

/// The prefix `/playFrom\(\s*(-?[\d.]+)/` used by the timeline for bookmarks (not anchored at the end).
pub fn play_from_prefix_arg(cmd: &str) -> Option<&str> {
    let i = cmd.find("playFrom(")?;
    let rest = cmd[i + "playFrom(".len()..].trim_start_matches(text::is_js_space);
    let n = number_len(rest);
    (n > 0).then(|| &rest[..n])
}

/// The state of media `spid` at the animation time `anim_ms` (∞ = end of the slide). `looping` ("repeat until
/// stopped"): the media node does not end at command + duration.
pub fn media_state_at(
    arena: &TimeArena,
    effects: &[PlacedEffect],
    spid: &str,
    anim_ms: f64,
    clock: &MediaClock,
    looping: bool,
) -> MediaPlayState {
    let k = clock.scale.unwrap_or(1.0);
    let lat = clock.latency_ms.unwrap_or(0.0);
    let (mut started, mut playing) = (false, false);
    let (mut pos, mut since) = (0.0_f64, 0.0_f64);
    let mut dur_ms: Option<f64> = None;
    let mut pos0 = 0.0_f64;
    for c in media_commands(arena, effects, spid) {
        if c.at_ms > anim_ms {
            break;
        }
        if let Some(m) = play_from_arg(&c.cmd) {
            started = true;
            playing = true;
            pos = math::max(0.0, parse::parse_float(m));
            since = c.at_ms + lat;
            pos0 = pos;
            if c.dur_ms.is_some() {
                dur_ms = c.dur_ms;
            }
            continue;
        }
        match &*c.cmd {
            "pause" => {
                if playing {
                    pos += (math::max(0.0, c.at_ms - since) * k) / 1000.0;
                    playing = false;
                }
            }
            "resume" => {
                if started && !playing {
                    playing = true;
                    since = c.at_ms + lat;
                }
            }
            "togglePause" => {
                if playing {
                    pos += (math::max(0.0, c.at_ms - since) * k) / 1000.0;
                    playing = false;
                } else if started {
                    playing = true;
                    since = c.at_ms + lat;
                }
            }
            "stop" => {
                started = false;
                playing = false;
                pos = 0.0;
            }
            _ => {}
        }
    }
    let mut time_sec = if playing { pos + (math::max(0.0, anim_ms - since) * k) / 1000.0 } else { pos };
    // the media node ends at command + duration (on the media clock): freezes there. Counts what PLAYED since
    // the playFrom (+ the start latency) — time paused does not consume the duration
    let mut ended = false;
    if started
        && !looping
        && let Some(d) = dur_ms
        && (time_sec - pos0) * 1000.0 + lat * k >= d
    {
        playing = false;
        ended = true;
        time_sec = math::min(time_sec, pos0 + (d - lat * k) / 1000.0);
    }
    MediaPlayState { started, playing, time_sec, dur_ms, ended: ended.then_some(true) }
}

/// The opacity of the shape `spid` (the viewer's `shapeOpacity`): 0 for a media with `showWhenStopped === false`
/// that is not playing, else 1. `info` is the media's entry of the timing model.
pub fn media_shape_opacity(
    arena: &TimeArena,
    effects: &[PlacedEffect],
    model: &TimingModel,
    spid: &str,
    anim_ms: f64,
    clock: &MediaClock,
) -> f64 {
    let Some(mi) = model.media.as_deref().and_then(|m| m.iter().find(|x| &*x.spid == spid)) else { return 1.0 };
    if mi.show_when_stopped == Some(false) {
        let st = media_state_at(arena, effects, spid, anim_ms, clock, mi.loop_ == Some(true));
        if !st.playing {
            return 0.0;
        }
    }
    1.0
}

/// Opacity of the video with fade (`p14:fade`): the video appears/disappears OVER the cover. `fade` = (in, out) ms.
pub fn media_fade_alpha(fade: Option<(f64, f64)>, played_ms: f64, len_ms: Option<f64>) -> f64 {
    let Some((in_ms, out_ms)) = fade else { return 1.0 };
    let mut a = 1.0;
    if in_ms > 0.0 {
        a = math::min(a, math::max(0.0, played_ms / in_ms));
    }
    if out_ms > 0.0
        && let Some(l) = len_ms
        && l.is_finite()
    {
        a = math::min(a, math::max(0.0, (l - played_ms) / out_ms));
    }
    a
}

/// The frame of an animated GIF after `elapsed_ms`. `delays_ms` = each frame's delay (≤ 10 ms means 100 ms, as
/// in browsers and the application); `repeat` = repetitions BEYOND the first pass (∞ = forever). After the last,
/// it stays on the last frame.
pub fn gif_frame_at(delays_ms: &[f64], elapsed_ms: f64, repeat: f64) -> usize {
    let n = delays_ms.len();
    if n <= 1 || elapsed_ms.is_nan() || elapsed_ms <= 0.0 {
        return 0;
    }
    let d: Vec<f64> = delays_ms.iter().map(|x| if *x > 10.0 { *x } else { 100.0 }).collect();
    let total: f64 = d.iter().sum();
    let round = math::floor(elapsed_ms / total);
    if round > repeat {
        return n - 1;
    }
    let mut t = elapsed_ms - round * total;
    for (i, di) in d.iter().enumerate() {
        if t < *di {
            return i;
        }
        t -= di;
    }
    n - 1
}
