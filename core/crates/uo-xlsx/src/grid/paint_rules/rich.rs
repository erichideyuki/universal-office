//! `layoutRichLines` (`grid-layout.ts`): word wrap of rich-text runs into lines that fit `max_w`, keeping the source run
//! of each segment. Rules (as the cell wrap): break at spaces, spaces do not start a line, a word longer than the line
//! breaks by character (code point), `\n` forces a break; it always returns at least one (possibly empty) line.

/// A segment of a line: a slice of the text of run `run` (UTF-16).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RichSeg {
    /// The text.
    pub text: Vec<u16>,
    /// The run it came from (for style and measurement).
    pub run: usize,
}

enum Tok {
    Newline,
    Word { start: usize, end: usize, run: usize },
}

/// `texts.split(/(\n| +)/)` pieces of one run, non-empty ones only, as ranges.
fn tokenize(text: &[u16], run: usize, out: &mut Vec<Tok>) {
    let mut i = 0;
    let mut word_start = 0;
    let flush = |from: usize, to: usize, out: &mut Vec<Tok>| {
        if to > from {
            out.push(Tok::Word { start: from, end: to, run });
        }
    };
    while i < text.len() {
        let u = text.get(i).copied().unwrap_or(0);
        if u == 10 {
            flush(word_start, i, out);
            out.push(Tok::Newline);
            i += 1;
            word_start = i;
        } else if u == 32 {
            flush(word_start, i, out);
            let s = i;
            while text.get(i) == Some(&32) {
                i += 1;
            }
            out.push(Tok::Word { start: s, end: i, run });
            word_start = i;
        } else {
            i += 1;
        }
    }
    flush(word_start, text.len(), out);
}

/// The code points of a UTF-16 slice as ranges (a lone surrogate is one unit).
fn code_points(t: &[u16]) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut i = 0;
    std::iter::from_fn(move || {
        let u = *t.get(i)?;
        let s = i;
        i += if (0xD800..0xDC00).contains(&u) && t.get(i + 1).is_some_and(|l| (0xDC00..0xE000).contains(l)) { 2 } else { 1 };
        Some((s, i))
    })
}

/// `layoutRichLines(texts, maxW, measure)`; `measure(text, run)` is the width of `text` in the font of `run`.
pub fn layout_rich_lines(texts: &[&[u16]], max_w: f64, measure: &mut dyn FnMut(&[u16], usize) -> f64) -> Vec<Vec<RichSeg>> {
    let mut toks: Vec<Tok> = Vec::new();
    for (r, t) in texts.iter().enumerate() {
        tokenize(t, r, &mut toks);
    }
    let mut lines: Vec<Vec<RichSeg>> = Vec::new();
    let mut line: Vec<RichSeg> = Vec::new();
    let mut line_w = 0.0f64;

    fn push_seg(line: &mut Vec<RichSeg>, line_w: &mut f64, text: &[u16], run: usize, w: f64) {
        match line.last_mut() {
            Some(last) if last.run == run => last.text.extend_from_slice(text),
            _ => line.push(RichSeg { text: text.to_vec(), run }),
        }
        *line_w += w;
    }

    // a word wider than a line: by code point
    #[allow(clippy::too_many_arguments)]
    fn split_word(
        word: &[u16],
        run: usize,
        max_w: f64,
        measure: &mut dyn FnMut(&[u16], usize) -> f64,
        lines: &mut Vec<Vec<RichSeg>>,
        line: &mut Vec<RichSeg>,
        line_w: &mut f64,
    ) {
        let mut chunk: Vec<u16> = Vec::new();
        for (s, e) in code_points(word) {
            let ch = word.get(s..e).unwrap_or(&[]);
            let was_empty = chunk.is_empty();
            let keep = chunk.len();
            chunk.extend_from_slice(ch);
            let cw = measure(&chunk, run);
            if !(cw <= max_w || was_empty) {
                chunk.truncate(keep);
                let w = measure(&chunk, run);
                push_seg(line, line_w, &chunk, run, w);
                lines.push(std::mem::take(line));
                *line_w = 0.0;
                chunk.clear();
                chunk.extend_from_slice(ch);
            }
        }
        if !chunk.is_empty() {
            let w = measure(&chunk, run);
            push_seg(line, line_w, &chunk, run, w);
        }
    }

    for tok in &toks {
        let Tok::Word { start, end, run } = *tok else {
            lines.push(std::mem::take(&mut line));
            line_w = 0.0;
            continue;
        };
        let word = texts.get(run).and_then(|t| t.get(start..end)).unwrap_or(&[]);
        let is_space = word.iter().all(|&u| u == 32);
        let w = measure(word, run);
        if line_w + w <= max_w || line.is_empty() {
            if line.is_empty() && is_space {
                continue;
            }
            if line.is_empty() && w > max_w {
                split_word(word, run, max_w, measure, &mut lines, &mut line, &mut line_w);
                continue;
            }
            push_seg(&mut line, &mut line_w, word, run, w);
            continue;
        }
        lines.push(std::mem::take(&mut line));
        line_w = 0.0;
        if is_space {
            continue;
        }
        if w > max_w {
            split_word(word, run, max_w, measure, &mut lines, &mut line, &mut line_w);
        } else {
            push_seg(&mut line, &mut line_w, word, run, w);
        }
    }
    lines.push(line);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn lines(texts: &[&str], w: f64) -> Vec<Vec<(String, usize)>> {
        let t: Vec<Vec<u16>> = texts.iter().map(|s| u(s)).collect();
        let r: Vec<&[u16]> = t.iter().map(Vec::as_slice).collect();
        let out = layout_rich_lines(&r, w, &mut |s, _| s.len() as f64 * 10.0);
        out.into_iter().map(|l| l.into_iter().map(|s| (String::from_utf16_lossy(&s.text), s.run)).collect()).collect()
    }

    #[test]
    fn wraps_at_spaces_and_keeps_runs() {
        let l = lines(&["aaa bbb ", "ccc ddd"], 80.0);
        assert_eq!(l, vec![vec![("aaa bbb ".to_owned(), 0)], vec![("ccc ddd".to_owned(), 1)]]);
        let l = lines(&["aaa ", "bbb ccc"], 80.0);
        assert_eq!(l, vec![vec![("aaa ".to_owned(), 0), ("bbb ".to_owned(), 1)], vec![("ccc".to_owned(), 1)]]);
    }

    #[test]
    fn newline_empty_and_long_words() {
        assert_eq!(lines(&["a\nb"], 100.0), vec![vec![("a".to_owned(), 0)], vec![("b".to_owned(), 0)]]);
        assert_eq!(lines(&[""], 10.0), vec![vec![]]);
        let l = lines(&["abcdefg"], 30.0);
        assert_eq!(l.iter().map(|x| x[0].0.as_str()).collect::<Vec<_>>(), vec!["abc", "def", "g"]);
        // leading spaces never start a line
        assert_eq!(lines(&["  x"], 100.0), vec![vec![("x".to_owned(), 0)]]);
    }
}
