//! Gate 2, D1: `glyph_ids_for` bisects the segments or groups of the cmap when they are in order
//! (the same segment as the TS linear scan) and keeps the scan, with the work budget, otherwise.
//!
//! The reference here is a transcription of the TS scan over the font description (not the
//! crate's code), so a result that differs is a different segment, not a shared bug. A "linear
//! twin" of a sorted font has one inert range appended (it breaks the order and can never be the
//! first match), which forces the scan: the pair must give the same ids, and the budget tells the
//! two paths apart.
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;

use b_support::*;
use uo_fonts::{FontError, FontLimits, LimitKind, glyph_ids_for};
use uo_testkit::generate::Rng;

fn cmap_font(subtable: Vec<u8>, encoding: u16) -> Vec<u8> {
    Font::basic().set(b"cmap", cmap(&[(3, encoding, subtable)])).build()
}

fn ids(bytes: &[u8], cps: &[u32], work: u64) -> Result<Vec<u32>, FontError> {
    glyph_ids_for(bytes, cps.iter().copied(), &FontLimits { max_table_work: work, ..FontLimits::NATIVE })
}

fn cps4() -> Vec<u32> {
    (0..=0x1_0001u32).chain([0x1_F600, u32::MAX]).collect()
}

/// A format-4 font: segments, with a glyph array for the `Array` ones.
struct Font4 {
    segs: Vec<Seg>,
    gids: Vec<u16>,
}

impl Font4 {
    fn bytes(&self) -> Vec<u8> {
        cmap_font(format4(&self.segs, &self.gids, None), 1)
    }

    /// The TS scan: the first segment whose end is not below the code point, then the checks.
    fn reference(&self, cp: u32) -> u32 {
        if cp > 0xFFFF {
            return 0;
        }
        for s in &self.segs {
            if cp > u32::from(s.end) {
                continue;
            }
            if cp < u32::from(s.start) {
                return 0;
            }
            return match s.ro {
                Ro::Zero => ((i64::from(cp) + i64::from(s.delta)) & 0xFFFF) as u32,
                Ro::Array(index) => {
                    let g = self.gids[usize::from(index) + (cp - u32::from(s.start)) as usize];
                    if g == 0 { 0 } else { ((i64::from(g) + i64::from(s.delta)) & 0xFFFF) as u32 }
                }
                Ro::Raw(_) => unreachable!(),
            };
        }
        0
    }

    fn expected(&self, cps: &[u32]) -> Vec<u32> {
        let mut v: Vec<u32> = cps.iter().map(|&c| self.reference(c)).filter(|&g| g != 0).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// The same font with one segment that breaks the order of the ends and can never be reached
    /// (its end 0 is below every code point the earlier segments did not take).
    fn linear_twin(&self) -> Font4 {
        let mut segs = self.segs.clone();
        segs.push(seg(5, 0, 0));
        Font4 { segs, gids: self.gids.clone() }
    }
}

fn random_font4(rng: &mut Rng, sorted_ends: bool) -> Font4 {
    let count = 40 + rng.below(260);
    let (mut segs, mut gids) = (Vec::new(), Vec::new());
    let mut end = 1u32;
    for _ in 0..count {
        end = if sorted_ends { (end + rng.below(250) as u32).min(0xFFFF) } else { rng.below(0x1_0000) as u32 };
        let start = if rng.one_in(12) { end + 1 + rng.below(5) as u32 } else { end.saturating_sub(rng.below(70) as u32) };
        let (start, end16) = (start.min(0xFFFF) as u16, end as u16);
        let delta = rng.next_u32() as i16;
        if rng.one_in(3) && start <= end16 {
            let index = gids.len() as u16;
            for _ in start..=end16 {
                gids.push(if rng.one_in(4) { 0 } else { rng.next_u32() as u16 });
            }
            segs.push(seg_array(start, end16, delta, index));
        } else {
            segs.push(seg(start, end16, delta));
        }
    }
    Font4 { segs, gids }
}

#[test]
fn format4_sorted_ends_equal_the_scan_overlaps_and_duplicates_included() {
    let cps = cps4();
    let mut rng = Rng::new(0xD1_0004);
    for _ in 0..40 {
        let font = random_font4(&mut rng, true);
        let want = font.expected(&cps);
        assert_eq!(ids(&font.bytes(), &cps, u64::MAX), Ok(want.clone()), "bisection");
        assert_eq!(ids(&font.linear_twin().bytes(), &cps, u64::MAX), Ok(want), "linear twin");
    }
}

#[test]
fn format4_unsorted_ends_take_the_scan_with_the_ts_result() {
    let cps = cps4();
    let mut rng = Rng::new(0xD1_0005);
    for _ in 0..40 {
        let font = random_font4(&mut rng, false);
        assert_eq!(ids(&font.bytes(), &cps, u64::MAX), Ok(font.expected(&cps)));
    }
}

fn groups12(rng: &mut Rng, sorted: bool) -> Vec<(u32, u32, u32)> {
    let count = 30 + rng.below(200);
    let mut groups = Vec::new();
    let mut next = 0u32;
    for _ in 0..count {
        let (a, b) = if sorted {
            let a = next + 1 + rng.below(40) as u32;
            (a, a + rng.below(30) as u32)
        } else {
            let a = rng.below(6000) as u32;
            if rng.one_in(10) { (a + 3, a) } else { (a, a + rng.below(300) as u32) }
        };
        next = b;
        groups.push((a, b, rng.below(60_000) as u32 + 1));
    }
    if sorted {
        groups.push((0xFFFF_FFF0, 0xFFFF_FFFF, 7));
    }
    groups
}

fn reference12(groups: &[(u32, u32, u32)], cp: u32) -> u64 {
    for &(a, b, g) in groups {
        if (a..=b).contains(&cp) {
            return u64::from(g) + u64::from(cp - a);
        }
    }
    0
}

fn expected12(groups: &[(u32, u32, u32)], cps: &[u32]) -> Vec<u32> {
    let mut v: Vec<u32> = cps.iter().map(|&c| u32::try_from(reference12(groups, c)).unwrap_or(u32::MAX)).filter(|&g| g != 0).collect();
    v.sort_unstable();
    v.dedup();
    v
}

#[test]
fn format12_sorted_disjoint_equal_the_scan_and_the_rest_takes_it() {
    let mut cps: Vec<u32> = (0..8000u32).collect();
    cps.extend([0xFFFF_FFEF, 0xFFFF_FFF0, 0xFFFF_FFF5, u32::MAX, 0x1_F600]);
    let mut rng = Rng::new(0xD1_0012);
    for sorted in [true, false] {
        for _ in 0..40 {
            let groups = groups12(&mut rng, sorted);
            let want = expected12(&groups, &cps);
            assert_eq!(ids(&cmap_font(format12(&groups), 10), &cps, u64::MAX), Ok(want.clone()), "sorted: {sorted}");
            let mut twin = groups.clone();
            twin.push((1, 0, 0));
            assert_eq!(ids(&cmap_font(format12(&twin), 10), &cps, u64::MAX), Ok(want), "linear twin, sorted: {sorted}");
        }
    }
}

#[test]
fn format12_touching_or_overlapping_groups_are_not_disjoint_and_keep_the_first_match() {
    let touching = [(10, 20, 100), (20, 30, 200)];
    let bytes = cmap_font(format12(&touching), 10);
    assert_eq!(ids(&bytes, &[20], u64::MAX), Ok(vec![110]), "the first group containing 20, as the scan");
    assert_eq!(ids(&bytes, &[21], u64::MAX), Ok(vec![201]));
    assert_eq!(ids(&bytes, &[20], 1), Ok(vec![110]), "the scan: one unit for the first group");
    let empty_first = [(9, 8, 1), (10, 20, 100)];
    assert_eq!(ids(&cmap_font(format12(&empty_first), 10), &[8, 9, 10, 15], u64::MAX), Ok(vec![100, 105]));
}

fn least_budget(bytes: &[u8], cps: &[u32]) -> u64 {
    (0..).find(|&work| ids(bytes, cps, work).is_ok()).unwrap()
}

#[test]
fn the_budget_counts_probes_on_the_bisection_and_steps_on_the_scan() {
    let sorted: Vec<(u32, u32, u32)> = (0..1000).map(|i| (i * 4, i * 4 + 1, i + 1)).collect();
    let bytes = cmap_font(format12(&sorted), 10);
    let mut unsorted = sorted.clone();
    unsorted.push((0, 1, 5000));
    let scan = cmap_font(format12(&unsorted), 10);
    for cp in [0u32, 1, 2, 80, 2000, 3996, 3997, 3999, 4000, u32::MAX] {
        let want = ids(&bytes, &[cp], u64::MAX);
        assert_eq!(ids(&scan, &[cp], u64::MAX), want, "cp {cp}");
        let probes = least_budget(&bytes, &[cp]);
        assert!((9..=10).contains(&probes), "cp {cp}: {probes} probes bisect 1,000 groups");
    }
    assert_eq!(least_budget(&bytes, &[0, 0, 0]), 30, "one budget for the whole call: 3 x 10 probes");
    assert_eq!(least_budget(&scan, &[3996]), 1000, "the scan visits 1,000 groups");
    assert_eq!(least_budget(&scan, &[3999]), 1001, "a miss visits all 1,001");
    assert_eq!(least_budget(&scan, &[80]), 21);
    assert_eq!(ids(&scan, &[3996], 999), Err(FontError::Limit(LimitKind::TableWork)));
}

#[test]
fn a_sorted_cjk_sized_cmap_passes_the_budget_where_the_scan_does_not() {
    let segs: Vec<Seg> = (0..20_000u16).map(|i| seg(i, i, 1)).collect();
    let font = Font4 { segs, gids: Vec::new() };
    let cps: Vec<u32> = (0..12_544u32).collect();
    let bytes = font.bytes();
    let budget = FontLimits::NATIVE.max_table_work;
    let sorted = ids(&bytes, &cps, budget).unwrap();
    assert_eq!(sorted, font.expected(&cps));
    assert_eq!(
        ids(&font.linear_twin().bytes(), &cps, budget),
        Err(FontError::Limit(LimitKind::TableWork)),
        "the scan costs about 7.9 x 10^7 here"
    );
    assert_eq!(ids(&font.linear_twin().bytes(), &cps, u64::MAX), Ok(sorted));
}

#[test]
fn a_cut_table_keeps_the_scan_and_its_errors() {
    let mut bytes = cmap_font(format12(&[(0, 5, 1)]), 10);
    let cmap_offset = (0..u16::from_be_bytes([bytes[4], bytes[5]]) as usize)
        .map(|i| 12 + i * 16)
        .find(|&p| &bytes[p..p + 4] == b"cmap")
        .map(|p| u32::from_be_bytes([bytes[p + 8], bytes[p + 9], bytes[p + 10], bytes[p + 11]]) as usize)
        .unwrap();
    let group_end = cmap_offset + 12 + 16 + 12;
    let group_count_at = cmap_offset + 12 + 12;
    bytes[group_count_at..group_count_at + 4].copy_from_slice(&50_000u32.to_be_bytes());
    bytes.truncate(group_end);
    assert_eq!(ids(&bytes, &[3], u64::MAX), Ok(vec![4]), "a match before the cut is answered");
    assert_eq!(ids(&bytes, &[3, 9], u64::MAX), Err(FontError::OutOfBounds), "the first read past the end throws");
    assert_eq!(ids(&bytes, &[], u64::MAX), Ok(vec![]));
}

#[test]
fn no_code_point_reads_nothing_and_a_format_other_than_4_and_12_finds_nothing() {
    let bytes = cmap_font(format12(&[(0, 5, 1)]), 10);
    assert_eq!(ids(&bytes, &[], 0), Ok(vec![]));
    let mut other = format12(&[(0, 5, 1)]);
    other[1] = 6;
    assert_eq!(ids(&cmap_font(other, 10), &[1], 1), Ok(vec![]));
}
