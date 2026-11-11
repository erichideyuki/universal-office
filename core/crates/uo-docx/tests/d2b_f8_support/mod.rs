//! Shared helpers of the D2b-F8 tests (record `core/docs/modules/uo-docx-d2b/F8.md`): the hex and
//! digest helpers of the captures, a fake shell answering the clipboard jobs, payload builders and
//! the seeded generators of clipboard commands.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use uo_docx::session::command::ClipImage;
use uo_docx::session::ops::clipboard::tree::{HtmlCell, HtmlTable, PasteHtml, TreeBuilder, encode_payload};
use uo_docx::session::ops::clipboard::{ClipboardCmd, PasteLayoutIn};
use uo_docx::session::{Command, Job, JobResult, Outcome, Session};
use uo_docx::text::Utf16;
use uo_testkit::Digest;

pub fn units(hex: &str) -> Vec<u16> {
    hex.split('.').filter(|h| !h.is_empty()).map(|h| u16::from_str_radix(h, 16).unwrap()).collect()
}

pub fn hx(u: &[u16]) -> String {
    u.iter().map(|c| format!("{c:x}")).collect::<Vec<_>>().join(".")
}

pub fn hs(s: &str) -> String {
    hx(&s.encode_utf16().collect::<Vec<_>>())
}

/// `sha(t)` of the captures: the first 32 hex digits of SHA-256 over the UTF-8 of `t`.
pub fn sha(t: &str) -> String {
    Digest::of(t.as_bytes()).short()
}

/// A 1×1 PNG.
pub const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63,
    0x60, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE5, 0x27, 0xDE, 0xFC, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60,
    0x82,
];

/// The base64 of [`PNG`] (a `data:` URL payload).
pub const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgAAAAAgAB5SfeAAAAAElFTkSuQmCC";

/// A paste-tree payload of a small HTML: paragraphs, emphasis, a list, a link and a picture.
pub fn sample_html() -> Vec<u8> {
    let mut b = TreeBuilder::body();
    b.open("H1", &[]).text("Title").close();
    b.open("P", &[("style", "color: red"), ("color", "rgb(255, 0, 0)")])
        .text("Hello ")
        .open("B", &[])
        .text("bold")
        .close()
        .text(" world")
        .close();
    b.open("UL", &[]);
    b.open("LI", &[]).text("one").close();
    b.open("LI", &[]).text("two").close();
    b.close();
    b.open("P", &[]).open("A", &[("href", "https://example.com")]).text("link").close().close();
    let src = format!("data:image/png;base64,{PNG_B64}");
    b.open("P", &[]).open("IMG", &[("src", &src), ("width", "20")]).close().close();
    let h = PasteHtml { raw: Utf16::from("<h1>Title</h1><p>Hello</p>"), fragment: b.finish(), tables: Vec::new() };
    encode_payload(&h)
}

/// A paste-tree payload of an HTML table (tables only: the table path of `pasteData`).
pub fn table_html(rows: usize, cols: usize) -> Vec<u8> {
    let mut b = TreeBuilder::body();
    b.open("TABLE", &[]).open("TBODY", &[]);
    let mut t = HtmlTable::default();
    for r in 0..rows {
        b.open("TR", &[]);
        let mut row = Vec::new();
        for c in 0..cols {
            b.open("TD", &[]).text(&format!("r{r}c{c}")).close();
            row.push(HtmlCell { th: r == 0, colspan: None, rowspan: None, paras: vec![Utf16::from(format!(" r{r}c{c} ").as_str())] });
        }
        b.close();
        t.rows.push(row);
    }
    let h = PasteHtml { raw: Utf16::from("<table><tr><td>x</td></tr></table>"), fragment: b.finish(), tables: vec![t] };
    encode_payload(&h)
}

/// How the fake shell answers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// the clipboard holds `text` (and the HTML of [`sample_html`] when asked)
    Plain,
    /// failures and garbage
    Hostile,
}

/// Answers every job of an outcome (a few rounds), returning the last outcome.
pub fn answer(
    s: &mut Session,
    cx: &mut super::d2a_support::Cx,
    mut out: Outcome,
    shell: Shell,
    text: &str,
    html: Option<Vec<u8>>,
    round: u32,
) -> Outcome {
    for _ in 0..8 {
        let Outcome::NeedsJobs(jobs) = out else { return out };
        if jobs.is_empty() {
            return Outcome::NeedsJobs(jobs);
        }
        let mut last = Outcome::NeedsJobs(Vec::new());
        for j in jobs {
            let r = match (&j, shell) {
                (Job::ReadClipboard { .. }, Shell::Plain) => JobResult::Clipboard {
                    text: Some(Utf16::from(text)),
                    html_tree: html.clone().map(Arc::from),
                    fragment: None,
                    image: None,
                },
                (Job::ReadClipboard { .. }, Shell::Hostile) => match round % 4 {
                    0 => JobResult::Failed,
                    1 => JobResult::Bytes(Arc::from(vec![1u8, 2, 3])),
                    2 => JobResult::Clipboard {
                        text: None,
                        html_tree: Some(Arc::from(vec![0x55u8, 0x4f, 0x50, 0x54, 9, 9])),
                        fragment: None,
                        image: Some(ClipImage { ext: "png".into(), data: Arc::from(PNG) }),
                    },
                    _ => JobResult::Image { width: 1, height: 1, rgba: None },
                },
                (Job::RenderClip { .. }, Shell::Plain) => {
                    JobResult::Rendered { png: Arc::from(PNG), width_twips: 1440.0, height_twips: 720.0 }
                }
                (Job::RenderClip { .. }, Shell::Hostile) => match round % 3 {
                    0 => JobResult::Failed,
                    1 => JobResult::Rendered { png: Arc::from(Vec::new()), width_twips: f64::NAN, height_twips: f64::INFINITY },
                    _ => JobResult::Rendered { png: Arc::from(PNG), width_twips: 1e300, height_twips: -5.0 },
                },
                (Job::DecodeImage { .. }, _) => JobResult::Image { width: 1, height: 1, rgba: Some(Arc::from(vec![0u8, 0, 0, 255])) },
                (Job::EncodeImage { .. }, _) => JobResult::Bytes(Arc::from(PNG)),
            };
            last = match s.complete(j.id(), r, &mut cx.cx()) {
                Ok(o) => o,
                Err(_) => Outcome::NeedsJobs(Vec::new()),
            };
        }
        out = last;
    }
    out
}

pub fn layout() -> PasteLayoutIn {
    PasteLayoutIn::default()
}

pub fn cmd(c: ClipboardCmd) -> Command {
    Command::Clipboard(c)
}

// ── seeded commands (memory, no-panic) ──

use uo_docx::session::DocPosition;
use uo_docx::session::ops::clipboard::{CellsPasteMode, ListPasteMode, PasteDefaults, PasteMode, PasteSpecialFormat, RichPick};
use uo_docx::session::ops::tables::{TableCmd, TableLayoutIn, TableRect};
use uo_docx::session::ops::text::TextCmd;
use uo_testkit::generate::Rng;

const MODES: [PasteMode; 5] = [PasteMode::Keep, PasteMode::Merge, PasteMode::Text, PasteMode::Picture, PasteMode::Default];
const LISTS: [ListPasteMode; 4] = [ListPasteMode::Continue, ListPasteMode::New, ListPasteMode::Merge, ListPasteMode::NoMerge];
const FORMATS: [PasteSpecialFormat; 6] = [
    PasteSpecialFormat::Rtf,
    PasteSpecialFormat::Text,
    PasteSpecialFormat::Picture,
    PasteSpecialFormat::Html,
    PasteSpecialFormat::Unicode,
    PasteSpecialFormat::Image,
];
const CELLS: [CellsPasteMode; 4] = [CellsPasteMode::Overwrite, CellsPasteMode::Rows, CellsPasteMode::Nest, CellsPasteMode::Text];

fn pos(rng: &mut Rng, s: &Session) -> DocPosition {
    let Some(paras) = s.paras() else { return DocPosition::new(0, 0) };
    let para = rng.below(paras.len().max(1));
    let len = paras.get(para).map_or(0, uo_docx::session::text::para_length);
    DocPosition::new(u32::try_from(para).unwrap(), u32::try_from(rng.below(len + 1)).unwrap())
}

/// One seeded step: `None` = undo/redo (by the low bit of the second value).
pub fn f8_step(rng: &mut Rng, s: &Session, ring_ids: &[u32], hostile: bool) -> Option<Command> {
    let k = rng.below(100);
    let mode = |rng: &mut Rng| if rng.below(3) == 0 { None } else { Some(MODES[rng.below(5)]) };
    Some(match k {
        0..=9 => Command::Text(TextCmd::Type { text: Utf16::from(["a", "Olá ", " ", "x y", "😀"][rng.below(5)]) }),
        10..=21 => {
            let a = pos(rng, s);
            let f = if rng.below(3) == 0 { a } else { pos(rng, s) };
            Command::Text(TextCmd::SetSelection { anchor: a, focus: f })
        }
        22..=31 => cmd(ClipboardCmd::Copy),
        32..=35 => cmd(ClipboardCmd::Cut),
        36..=43 => cmd(ClipboardCmd::Paste { mode: mode(rng), layout: layout() }),
        44..=51 => cmd(ClipboardCmd::PasteData {
            text: Utf16::from(["", "a\nb", "x", "Title"][rng.below(4)]),
            html: match rng.below(4) {
                0 => None,
                1 => Some(Arc::from(sample_html())),
                2 => Some(Arc::from(table_html(1 + rng.below(3), 1 + rng.below(3)))),
                _ if hostile => Some(Arc::from((0..rng.below(64)).map(|_| u8::try_from(rng.below(256)).unwrap()).collect::<Vec<u8>>())),
                _ => Some(Arc::from(sample_html())),
            },
            image: if rng.below(6) == 0 { Some(ClipImage { ext: "png".into(), data: Arc::from(PNG) }) } else { None },
            rich: if rng.below(2) == 0 { RichPick::WithRich } else { RichPick::None },
            mode: mode(rng),
            list: if rng.below(3) == 0 { Some(LISTS[rng.below(4)]) } else { None },
            layout: PasteLayoutIn {
                content_width_twips: if hostile && rng.below(4) == 0 { Some([f64::NAN, -1.0, 1e300, 0.0][rng.below(4)]) } else { None },
            },
        }),
        52..=57 => cmd(ClipboardCmd::Repaste {
            mode: MODES[rng.below(5)],
            list: if rng.below(2) == 0 { Some(LISTS[rng.below(4)]) } else { None },
            layout: layout(),
        }),
        58..=61 => cmd(ClipboardCmd::PasteCells { mode: CELLS[rng.below(4)], layout: layout() }),
        62..=63 => cmd(ClipboardCmd::PasteSpecialFormats),
        64..=67 => cmd(ClipboardCmd::PasteSpecial { format: FORMATS[rng.below(6)], link: rng.below(2) == 0, layout: layout() }),
        68..=71 => {
            let id =
                if hostile && rng.below(3) == 0 { u32::MAX } else { ring_ids.get(rng.below(ring_ids.len().max(1))).copied().unwrap_or(0) };
            match rng.below(4) {
                0 => cmd(ClipboardCmd::RemoveClipboardItem { id }),
                1 => cmd(ClipboardCmd::PasteAllClipboardItems { layout: layout() }),
                _ => cmd(ClipboardCmd::PasteClipboardItem { id, mode: mode(rng), layout: layout() }),
            }
        }
        72 => cmd(ClipboardCmd::ClearClipboardItems),
        73 => cmd(ClipboardCmd::SetPasteDefaults {
            defaults: PasteDefaults { within: MODES[rng.below(5)], between: MODES[rng.below(5)], other: MODES[rng.below(5)] },
        }),
        74 => cmd(ClipboardCmd::SetDocumentPath { path: if rng.below(2) == 0 { Some(r"C:\a\b.docx".into()) } else { None } }),
        75 => cmd(ClipboardCmd::AddLinkSource { item: if rng.below(2) == 0 { None } else { ring_ids.first().copied() } }),
        76 => cmd(ClipboardCmd::PasteLinkWith { name: "OLE_LINK1".into(), item: None, text: rng.below(2) == 0, layout: layout() }),
        77..=79 => Command::Table(TableCmd::InsertTable {
            rows: 2.0,
            cols: 2.0,
            autofit: None,
            col_width_twips: None,
            layout: TableLayoutIn::default(),
        }),
        80..=83 => {
            let n = s.document().body.tables.len();
            if n == 0 {
                return Some(cmd(ClipboardCmd::Copy));
            }
            let (r2, c2) = (i64::try_from(rng.below(3)).unwrap(), i64::try_from(rng.below(3)).unwrap());
            Command::Table(TableCmd::SetTableSelection {
                anchor: u32::try_from(rng.below(n)).unwrap(),
                path: Vec::new(),
                rect: TableRect { r1: 0, c1: 0, r2, c2 },
                origin: (0, 0),
            })
        }
        84..=86 => Command::Settings(uo_docx::session::ops::settings::SettingsCmd::SetPolicy {
            policy: if rng.below(2) == 0 { Some(uo_docx::session::FormattingPolicy::template_document()) } else { None },
        }),
        87..=88 => cmd(ClipboardCmd::PasteText { text: Utf16::from(["a\r\nb", "", "\u{FFFC}x"][rng.below(3)]) }),
        _ => return None,
    })
}

/// Runs one step (the jobs answered by the shell), ignoring refusals.
pub fn run_f8(s: &mut Session, cx: &mut super::d2a_support::Cx, c: Option<Command>, rng: &mut Rng, shell: Shell, round: u32) {
    match c {
        None => {
            if rng.below(2) == 0 {
                s.undo(&mut cx.cx());
            } else {
                s.redo(&mut cx.cx());
            }
        }
        Some(c) => {
            if let Ok(o) = s.exec(c, &mut cx.cx()) {
                let html = if rng.below(2) == 0 { Some(sample_html()) } else { None };
                let o = answer(s, cx, o, shell, ["x", "", "a\nb"][rng.below(3)], html, round);
                if matches!(o, Outcome::NeedsJobs(_)) {
                    s.cancel_pending();
                }
            }
        }
    }
}
