//! The shape of a [`DocDiff`] for the shell and for parity: `JSON.stringify` of the TS `DocDiff`
//! (keys in the order the TS builds the objects: `kind, a, b, aBlocks, bBlocks`, then `formatOnly` and
//! `importance` on a format-only change, then `intra`, then `tableRows`). `limited` is not part of it.

use super::diff::{BlockRange, DocDiff, DocEdit, DocEditKind, DocIntraEdit, Importance, IntraKind, Side, TableRowEdit};

fn range(r: BlockRange, out: &mut String) {
    out.push_str(&format!("{{\"start\":{},\"end\":{}}}", r.start, r.end));
}

fn ints(v: &[usize], out: &mut String) {
    out.push('[');
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&x.to_string());
    }
    out.push(']');
}

fn kind(k: DocEditKind, out: &mut String) {
    out.push('"');
    out.push_str(k.as_str());
    out.push('"');
}

fn intra(m: &DocIntraEdit, out: &mut String) {
    out.push_str("{\"kind\":");
    out.push_str(match m.kind {
        IntraKind::Insert => "\"insert\"",
        IntraKind::Delete => "\"delete\"",
    });
    out.push_str(",\"side\":");
    out.push_str(match m.side {
        Side::A => "\"a\"",
        Side::B => "\"b\"",
    });
    out.push_str(&format!(",\"pairOffset\":{},\"startCh\":{},\"endCh\":{}}}", m.pair_offset, m.start_ch, m.end_ch));
}

fn row_edit(r: &TableRowEdit, out: &mut String) {
    out.push_str("{\"kind\":");
    kind(r.kind, out);
    out.push_str(",\"aRows\":");
    ints(&r.a_rows, out);
    out.push_str(",\"bRows\":");
    ints(&r.b_rows, out);
    if r.format_only {
        out.push_str(",\"formatOnly\":true");
    }
    out.push('}');
}

fn edit(e: &DocEdit, out: &mut String) {
    out.push_str("{\"kind\":");
    kind(e.kind, out);
    out.push_str(",\"a\":");
    range(e.a, out);
    out.push_str(",\"b\":");
    range(e.b, out);
    out.push_str(",\"aBlocks\":");
    ints(&e.a_blocks, out);
    out.push_str(",\"bBlocks\":");
    ints(&e.b_blocks, out);
    if e.format_only {
        out.push_str(",\"formatOnly\":true");
    }
    match e.importance {
        Some(Importance::Minor) => out.push_str(",\"importance\":\"minor\""),
        Some(Importance::Important) => out.push_str(",\"importance\":\"important\""),
        None => {}
    }
    if let Some(marks) = &e.intra {
        out.push_str(",\"intra\":[");
        for (i, m) in marks.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            intra(m, out);
        }
        out.push(']');
    }
    if let Some(tables) = &e.table_rows {
        out.push_str(",\"tableRows\":[");
        for (i, t) in tables.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            match t {
                None => out.push_str("null"),
                Some(rows) => {
                    out.push('[');
                    for (j, r) in rows.iter().enumerate() {
                        if j > 0 {
                            out.push(',');
                        }
                        row_edit(r, out);
                    }
                    out.push(']');
                }
            }
        }
        out.push(']');
    }
    out.push('}');
}

impl DocDiff {
    /// `JSON.stringify(diff)` of the TS `DocDiff`.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\"edits\":[");
        for (i, e) in self.edits.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            edit(e, &mut out);
        }
        let s = &self.stats;
        out.push_str(&format!(
            "],\"stats\":{{\"inserted\":{},\"deleted\":{},\"changed\":{},\"formatOnly\":{},\"equal\":{}}}}}",
            s.inserted, s.deleted, s.changed, s.format_only, s.equal
        ));
        out
    }
}
