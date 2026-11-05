//! Hostile F9 commands for the no-panic and memory tests: catastrophic patterns (exponential
//! backtracking, V8's endless empty-match loop before an astral character, deep nesting, huge
//! bounds), broken specials, lone surrogates, positions and indices at the ends of their types.

use uo_docx::session::Command;
use uo_docx::session::ops::find::engine::FindQuery;
use uo_docx::session::ops::find::{Direction, FindCmd, GoToWhat, Within};
use uo_docx::session::ops::navigation::{FindOptions, HeadingWhere, NavigationCmd};
use uo_docx::session::{DocPosition, DocRange};
use uo_testkit::generate::Rng;

use super::{REPLS, format};

/// Patterns whose search must stay inside the step budget.
pub fn hostile_patterns() -> Vec<(Vec<u16>, bool)> {
    let w = |s: &str| (s.encode_utf16().collect::<Vec<u16>>(), true);
    let p = |s: &str| (s.encode_utf16().collect::<Vec<u16>>(), false);
    let mut v = vec![
        w("(a@)@b"),
        w("(*)@x"),
        w("*a*a*a*a*a*a*a*a*z"),
        w("(?@)@(?@)@!"),
        w("((?)@)@#"),
        w("<{99999}"),
        w("(<@)"),
        w("(@)"),
        w("x{99999999999999999999}"),
        w("(a{0}){99999999}"),
        w("*"),
        w("<*>"),
        w("[😀-😁]"),
        w("\\😀@"),
        w("(*)(*)(*)(*)(*)(*)(*)(*)(*)\\9"),
        w("?@?@?@?@?@?@?@?@x"),
        w("[!a]@[!b]@[!c]@#"),
        p("e-mail"),
        p("^"),
        p("^u"),
        p("^1114112"),
        p("^55357^56832"),
        p("^w^w^w^w^w^w"),
        p("\u{0}"),
    ];
    v.push((format!("{}a{}", "(".repeat(3000), ")".repeat(3000)).encode_utf16().collect(), true));
    v.push((format!("{}a", "(?)@".repeat(400)).encode_utf16().collect(), true));
    v.push((vec![0xD83D; 300], false));
    v.push((vec![0xDE00, 0xD83D, 0x5B, 0x21, 0xD83D, 0x5D], true));
    v
}

const GOTO: [GoToWhat; 13] = [
    GoToWhat::Page,
    GoToWhat::Section,
    GoToWhat::Line,
    GoToWhat::Bookmark,
    GoToWhat::Comment,
    GoToWhat::Footnote,
    GoToWhat::Endnote,
    GoToWhat::Field,
    GoToWhat::Table,
    GoToWhat::Graphic,
    GoToWhat::Equation,
    GoToWhat::Object,
    GoToWhat::Heading,
];
const TARGETS: [&str; 12] =
    ["", "1", "+99999999999999999999", "-0", "-", "+", "1e9", "  7  ", "\u{0}", "PAGE", "999999999999", "-99999999999"];
const INDEX: [i64; 7] = [i64::MIN, -1, 0, 1, 2, 7, i64::MAX];

fn query(rng: &mut Rng) -> FindQuery {
    let pats = hostile_patterns();
    let (text, wild) = if rng.below(3) == 0 {
        let pool: [u16; 12] = [0x61, 0x20, 0x2D, 0x5E, 0x5C, 0x5B, 0x5D, 0x28, 0x29, 0x40, 0xD83D, 0x7B];
        ((0..rng.below(300)).map(|_| pool[rng.below(pool.len())]).collect(), rng.below(2) == 0)
    } else {
        pats[rng.below(pats.len())].clone()
    };
    FindQuery {
        text,
        wildcards: wild,
        match_case: rng.below(2) == 0,
        whole_word: rng.below(3) == 0,
        sounds_like: rng.below(9) == 0,
        all_forms: rng.below(9) == 0,
        prefix: rng.below(5) == 0,
        suffix: rng.below(5) == 0,
        ignore_punct: rng.below(4) == 0,
        ignore_space: rng.below(4) == 0,
        format: format(rng.below(24)).map(Box::new),
    }
}

fn pos(rng: &mut Rng) -> DocPosition {
    let v = [0, 1, 3, 7, 1000, u32::MAX];
    DocPosition::new(v[rng.below(v.len())], v[rng.below(v.len())])
}

/// One hostile F9 command.
pub fn hostile_f9(rng: &mut Rng) -> Command {
    let within = if rng.below(2) == 0 { Within::Document } else { Within::Selection };
    let dir = [Direction::Down, Direction::Up, Direction::All][rng.below(3)];
    let ri = rng.below(REPLS.len());
    let repl = || REPLS[ri].encode_utf16().collect::<Vec<u16>>();
    let ix = INDEX[rng.below(INDEX.len())];
    match rng.below(17) {
        0 | 1 => Command::Find(FindCmd::FindNext { query: query(rng), direction: dir }),
        2 => Command::Find(FindCmd::ReplaceMatch {
            query: query(rng),
            replacement: repl(),
            direction: dir,
            replace_format: format(rng.below(24)).map(Box::new),
        }),
        3 => Command::Find(FindCmd::ReplaceAll {
            query: query(rng),
            replacement: repl(),
            within,
            replace_format: format(rng.below(24)).map(Box::new),
        }),
        4 => Command::Find(FindCmd::SelectAllMatches { query: query(rng), within }),
        5 => Command::Find(FindCmd::HighlightMatches { query: query(rng), within }),
        6 => Command::Find(FindCmd::GoTo {
            what: GOTO[rng.below(GOTO.len())],
            target: TARGETS[rng.below(TARGETS.len())].encode_utf16().collect(),
        }),
        7 => {
            let names = [String::new(), "_Toc1".to_owned(), "x".repeat(300), "\u{0}".to_owned()];
            Command::Find(FindCmd::GoToBookmark { name: names[rng.below(4)].clone() })
        }
        8 => Command::Find(FindCmd::GoToComment { id: [f64::NAN, -0.0, 1e300, 0.0, 1.0][rng.below(5)] }),
        9 => Command::Navigation(NavigationCmd::SetSelectionRanges {
            ranges: (0..rng.below(40)).map(|_| DocRange::new(pos(rng), pos(rng))).collect(),
        }),
        10 => Command::Navigation(NavigationCmd::MoveHeading { index: ix, before: INDEX[rng.below(INDEX.len())] }),
        11 => Command::Navigation(if rng.below(2) == 0 {
            NavigationCmd::PromoteHeading { index: ix }
        } else {
            NavigationCmd::DemoteHeading { index: ix }
        }),
        12 => Command::Navigation(NavigationCmd::InsertHeading {
            index: ix,
            at: [HeadingWhere::Before, HeadingWhere::After, HeadingWhere::Sub][rng.below(3)],
        }),
        13 => Command::Navigation(NavigationCmd::DeleteHeading { index: ix }),
        14 => Command::Navigation(if rng.below(2) == 0 {
            NavigationCmd::SelectHeading { index: ix }
        } else {
            NavigationCmd::GoToHeading { index: ix }
        }),
        15 => Command::Navigation(NavigationCmd::Search {
            text: query(rng).text,
            opts: FindOptions { match_case: rng.below(2) == 0, whole_word: rng.below(2) == 0 },
        }),
        _ => Command::Navigation(match rng.below(3) {
            0 => NavigationCmd::GoToSearchResult { index: ix },
            1 => NavigationCmd::SelectAll,
            _ => NavigationCmd::ClearSearch,
        }),
    }
}
