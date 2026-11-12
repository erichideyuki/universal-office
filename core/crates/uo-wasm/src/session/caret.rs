//! Keyboard caret moves (`moveCaret`, universal-write.ts 9956; `moveWord`, 9915; `lineEndOffset`,
//! 18624) over the session and the lines of the shell's last layout ([`EditLines`]). A move only
//! changes the selection (no undo step, no event, the dirty flag untouched): the result is the
//! session's change set after `set_selection_raw`.
//!
//! Declared (WS-D7): `tableNavigate` (the arrows between table cells, 9956's first line) and `moveCell`
//! are not ported — milestone-2 gate item (a Chromium capture at the editor switch); the moves here
//! have no capture of their own (unit tests only).

use uo_docx::session::text::{para_length, para_text};
use uo_docx::session::{ChangeSet, DocPosition, Selection, Session};
use uo_docx_layout::hit::{EditLine, EditLines, EditPos};

/// The direction of `uo_session_move_caret`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    WordLeft,
    WordRight,
    /// Ctrl+Home (wave 6, C): the story's start, assigned as the TS assigns `this.selection` (10606: no
    /// canonical position, the pending properties and the float selection kept).
    DocStart,
    /// Ctrl+End: the story's end (the last paragraph's length), as [`Dir::DocStart`].
    DocEnd,
}

impl Dir {
    #[must_use]
    pub fn from_code(c: u32) -> Option<Dir> {
        Some(match c {
            0 => Dir::Left,
            1 => Dir::Right,
            2 => Dir::Up,
            3 => Dir::Down,
            4 => Dir::Home,
            5 => Dir::End,
            6 => Dir::WordLeft,
            7 => Dir::WordRight,
            8 => Dir::DocStart,
            9 => Dir::DocEnd,
            _ => return None,
        })
    }

    /// Whether the move reads the layout's lines.
    #[must_use]
    pub fn needs_lines(self) -> bool {
        matches!(self, Dir::Up | Dir::Down | Dir::Home | Dir::End)
    }
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn plen(s: &Session, i: u32) -> u32 {
    s.paras().and_then(|ps| ps.get(i as usize)).map_or(0, |p| u32_of(para_length(p)))
}

fn para_count(s: &Session) -> u32 {
    s.paras().map_or(0, |ps| u32_of(ps.len()))
}

/// `/\s/` of one UTF-16 code unit (the JS white space and line terminators).
fn is_space(u: u16) -> bool {
    uo_jsnum::text::is_js_space_unit(u)
}

/// The lines in the order of `allLines()` (paragraph by paragraph), each with its paragraph.
fn all_lines(el: &EditLines) -> Vec<(u32, &EditLine)> {
    el.paras.iter().enumerate().flat_map(|(i, p)| p.lines.iter().map(move |l| (u32_of(i), l))).collect()
}

/// `lineEndOffset(line)`: before the trailing white space of a wrapped line (its end is the next
/// line's start); the paragraph's last line ends at its end.
fn line_end_offset(s: &Session, el: &EditLines, para: u32, line: &EditLine) -> u32 {
    let last = el.paras.get(para as usize).and_then(|p| p.lines.last()).is_none_or(|l| std::ptr::eq(l, line));
    if last {
        return line.end;
    }
    let Some(p) = s.paras().and_then(|ps| ps.get(para as usize)) else { return line.end };
    let text = para_text(p);
    let mut e = line.end;
    while e > line.start && text.get(e as usize - 1).is_some_and(|&u| is_space(u)) {
        e -= 1;
    }
    e
}

/// `moveCaret(dir, extend)` (not the word moves): the new focus, `None` when the paragraphs are not
/// there (a stale position).
fn caret_focus(s: &Session, lines: Option<&EditLines>, dir: Dir) -> Option<DocPosition> {
    let sel = s.selection();
    let (mut para, mut offset) = (sel.focus.para, sel.focus.offset);
    let paras = s.paras()?;
    let hid = |i: u32, o: u32| s.hidden_char(paras.get(i as usize), o as usize);
    match dir {
        Dir::Left => {
            if offset > 0 {
                offset -= 1;
                while offset > 0 && hid(para, offset - 1) {
                    offset -= 1;
                }
            } else if para > 0 {
                para -= 1;
                offset = plen(s, para);
            }
        }
        Dir::Right => {
            while offset < plen(s, para) && hid(para, offset) {
                offset += 1;
            }
            if offset < plen(s, para) {
                offset += 1;
            } else if para + 1 < para_count(s) {
                para += 1;
                offset = 0;
            }
        }
        Dir::Home | Dir::End => {
            let el = lines?;
            if let Some(line) = el.line_of(EditPos { para, offset }) {
                offset = if dir == Dir::Home { line.start } else { line_end_offset(s, el, para, line) };
            }
        }
        Dir::Up | Dir::Down => {
            let el = lines?;
            if let Some(line) = el.line_of(EditPos { para, offset }) {
                let x = el.caret_x(offset, line);
                let all = all_lines(el);
                let idx = all.iter().position(|(_, l)| std::ptr::eq(*l, line));
                let target = idx.and_then(|i| if dir == Dir::Down { all.get(i + 1) } else { i.checked_sub(1).and_then(|j| all.get(j)) });
                if let Some(&(tp, tl)) = target {
                    let hit = el.hit_line_x(tl, x);
                    para = tp;
                    offset = hit;
                }
            }
        }
        Dir::WordLeft | Dir::WordRight | Dir::DocStart | Dir::DocEnd => return None,
    }
    Some(s.canon_pos(DocPosition { para, offset }))
}

/// `moveWord(dir, extend)`: the next word boundary from the focus.
fn word_focus(s: &Session, right: bool) -> Option<DocPosition> {
    let sel = s.selection();
    let (mut para, mut offset) = (sel.focus.para, sel.focus.offset);
    let paras = s.paras()?;
    let text = para_text(paras.get(para as usize)?);
    let len = u32_of(text.len());
    let at = |i: u32| text.get(i as usize).copied();
    if right {
        if offset >= len {
            if para + 1 < u32_of(paras.len()) {
                para += 1;
                offset = 0;
            }
        } else {
            while offset < len && at(offset).is_some_and(|u| !is_space(u)) {
                offset += 1;
            }
            while offset < len && at(offset).is_some_and(is_space) {
                offset += 1;
            }
        }
    } else if offset == 0 {
        if para > 0 {
            para -= 1;
            offset = plen(s, para);
        }
    } else {
        while offset > 0 && at(offset - 1).is_some_and(is_space) {
            offset -= 1;
        }
        while offset > 0 && at(offset - 1).is_some_and(|u| !is_space(u)) {
            offset -= 1;
        }
    }
    Some(DocPosition { para, offset })
}

/// Ctrl+Home/End (universal-write.ts 10606): `{ para: 0, offset: 0 }` or the last paragraph's end.
fn doc_focus(s: &Session, end: bool) -> Option<DocPosition> {
    let n = para_count(s);
    if n == 0 {
        return None;
    }
    Some(if end { DocPosition { para: n - 1, offset: plen(s, n - 1) } } else { DocPosition { para: 0, offset: 0 } })
}

/// The move: the new selection set (the anchor kept when `extend`), the pending properties cleared
/// for the caret moves (not for the word moves nor Ctrl+Home/End, as the TS), and the change set.
/// `None` when the session has no paragraphs in the context or the move needs lines it was not given.
pub fn move_caret(s: &mut Session, lines: Option<&EditLines>, dir: Dir, extend: bool) -> Option<ChangeSet> {
    let word = matches!(dir, Dir::WordLeft | Dir::WordRight);
    let doc = matches!(dir, Dir::DocStart | Dir::DocEnd);
    let focus = if word {
        word_focus(s, dir == Dir::WordRight)?
    } else if doc {
        doc_focus(s, dir == Dir::DocEnd)?
    } else {
        caret_focus(s, lines, dir)?
    };
    let sel = s.selection();
    s.set_selection_raw(if extend { Selection { anchor: sel.anchor, focus } } else { Selection { anchor: focus, focus } });
    if !word && !doc {
        s.set_pending_props(None);
    }
    Some(s.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uo_docx::session::ops::text::TextCmd;
    use uo_docx::session::{ClipboardRing, Command, ExecCx, FixedEnv, OpenOptions};
    use uo_docx::text::Utf16;
    use uo_docx::view::{DocRef, DocView, FieldScope, HyphenCandidate, HyphenSkips, LayoutError, LayoutOracle, RunRef};

    struct NoLayout;
    impl LayoutOracle for NoLayout {
        fn field_values(&mut self, _: DocView<'_>, _: &FieldScope) -> Result<Vec<(RunRef, Utf16)>, LayoutError> {
            Ok(Vec::new())
        }
        fn next_hyphenation(&mut self, _: DocView<'_>, _: &DocRef, _: &HyphenSkips) -> Result<Option<HyphenCandidate>, LayoutError> {
            Ok(None)
        }
    }

    fn session(text: &str) -> Session {
        let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap_or_else(|_| unreachable!());
        let mut ring = ClipboardRing::default();
        let mut lay = NoLayout;
        let mut cx = ExecCx { layout: &mut lay, clipboard: &mut ring };
        for (i, part) in text.split('\n').enumerate() {
            if i > 0 {
                let _ = s.exec(Command::Text(TextCmd::Enter), &mut cx);
            }
            let _ = s.exec(Command::Text(TextCmd::Type { text: Utf16::from(part) }), &mut cx);
        }
        s
    }

    fn at(s: &mut Session, para: u32, offset: u32) {
        let p = DocPosition { para, offset };
        s.set_selection_raw(Selection { anchor: p, focus: p });
    }

    #[test]
    fn left_right_and_words_without_a_layout() {
        let mut s = session("ab cd  ef\nx");
        let steps = s.history().undo_len();
        let dirty = s.is_dirty();
        at(&mut s, 0, 0);
        let cs = move_caret(&mut s, None, Dir::Right, false).unwrap_or_default();
        assert_eq!(s.selection().focus, DocPosition { para: 0, offset: 1 });
        assert!(cs.events.is_empty());
        assert_eq!(s.history().undo_len(), steps);
        assert_eq!(s.is_dirty(), dirty);
        move_caret(&mut s, None, Dir::WordRight, true);
        assert_eq!(s.selection().focus, DocPosition { para: 0, offset: 3 });
        assert_eq!(s.selection().anchor, DocPosition { para: 0, offset: 1 });
        move_caret(&mut s, None, Dir::WordRight, false);
        assert_eq!(s.selection().focus, DocPosition { para: 0, offset: 7 });
        move_caret(&mut s, None, Dir::WordRight, false);
        assert_eq!(s.selection().focus, DocPosition { para: 0, offset: 9 });
        move_caret(&mut s, None, Dir::WordRight, false);
        assert_eq!(s.selection().focus, DocPosition { para: 1, offset: 0 });
        move_caret(&mut s, None, Dir::Left, false);
        assert_eq!(s.selection().focus, DocPosition { para: 0, offset: 9 });
        move_caret(&mut s, None, Dir::WordLeft, false);
        assert_eq!(s.selection().focus, DocPosition { para: 0, offset: 7 });
        move_caret(&mut s, None, Dir::WordLeft, false);
        assert_eq!(s.selection().focus, DocPosition { para: 0, offset: 3 });
        at(&mut s, 1, 1);
        move_caret(&mut s, None, Dir::Right, false);
        assert_eq!(s.selection().focus, DocPosition { para: 1, offset: 1 });
        // Ctrl+End / Ctrl+Shift+Home: no layout, the anchor kept with Shift
        move_caret(&mut s, None, Dir::DocEnd, false);
        assert_eq!(s.selection().focus, DocPosition { para: 1, offset: 1 });
        move_caret(&mut s, None, Dir::DocStart, true);
        assert_eq!(s.selection().anchor, DocPosition { para: 1, offset: 1 });
        assert_eq!(s.selection().focus, DocPosition { para: 0, offset: 0 });
        // the line moves need a layout
        assert!(move_caret(&mut s, None, Dir::Up, false).is_none());
        assert_eq!(s.history().undo_len(), steps);
    }
}
