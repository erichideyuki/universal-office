//! Slide transition (`SSSlideInfoAtom`) → `<p:transition>` (`ppt/transition.ts`).
//!
//! Types, directions and the speed scale were checked against the COM manifest of PowerPoint
//! reading the .ppt and its twin .pptx: speed 0 = slow, 1 = medium, 2 = fast; blinds 0 = vertical,
//! 1 = horizontal; directions of cover/pull/push 0..7 = l, u, r, d, lu, ru, ld, rd.

use super::reader::PptTransition;
use super::xmlw::{b, el, n, s};

const DIR8: [&str; 8] = ["l", "u", "r", "d", "lu", "ru", "ld", "rd"];

fn dir8(d: u8) -> Option<&'static str> {
    DIR8.get(usize::from(d)).copied()
}

/// `<p:transition>` of the slide; empty when there is nothing to write.
#[must_use]
pub fn transition_xml(t: Option<&PptTransition>) -> String {
    let Some(t) = t else { return String::new() };
    let d = t.effect_direction;
    let not0 = |x: Option<&'static str>| if d == 0 { None } else { x.and_then(s) };
    let fx = match t.effect_type {
        0 => el("p:cut", vec![("thruBlk", b(d == 1))], ""),
        1 => "<p:random/>".to_owned(),
        2 => el("p:blinds", vec![("dir", if d == 0 { s("vert") } else { None })], ""),
        3 => el("p:checker", vec![("dir", if d == 1 { s("vert") } else { None })], ""),
        4 => el("p:cover", vec![("dir", not0(dir8(d)))], ""),
        5 => "<p:dissolve/>".to_owned(),
        6 => "<p:fade thruBlk=\"1\"/>".to_owned(),
        7 => el("p:pull", vec![("dir", not0(dir8(d)))], ""),
        8 => el("p:randomBar", vec![("dir", if d == 1 { s("vert") } else { None })], ""),
        9 => el("p:strips", vec![("dir", if d >= 4 { dir8(d).and_then(s) } else { None })], ""),
        10 => el("p:wipe", vec![("dir", not0(dir8(d)))], ""),
        11 => el("p:zoom", vec![("dir", if d == 1 { s("in") } else { None })], ""),
        13 => el(
            "p:split",
            vec![("orient", if d >= 2 { s("vert") } else { None }), ("dir", if d == 1 || d == 3 { s("in") } else { None })],
            "",
        ),
        17 => "<p:diamond/>".to_owned(),
        18 => "<p:plus/>".to_owned(),
        19 => "<p:wedge/>".to_owned(),
        20 => el("p:push", vec![("dir", not0(dir8(d)))], ""),
        21 => el("p:comb", vec![("dir", if d == 1 { s("vert") } else { None })], ""),
        22 => "<p:newsflash/>".to_owned(),
        23 => "<p:fade/>".to_owned(),
        26 => el("p:wheel", vec![("spokes", if d != 0 && d != 4 { n(d) } else { None })], ""),
        27 => "<p:circle/>".to_owned(),
        _ => String::new(),
    };
    let spd = match t.speed {
        0 => s("slow"),
        1 => s("med"),
        _ => None,
    };
    let adv_click = if t.manual_advance { None } else { n(0) };
    let adv_tm = if t.auto_advance { n(t.slide_time.max(0)) } else { None };
    if fx.is_empty() && adv_tm.is_none() && adv_click.is_none() {
        return String::new();
    }
    el("p:transition", vec![("spd", spd), ("advClick", adv_click), ("advTm", adv_tm)], &fx)
}
